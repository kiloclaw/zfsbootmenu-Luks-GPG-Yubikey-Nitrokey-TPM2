//! The use cases of LUKS volumes: unlock every volume, open one, with its
//! key methods or a key file alone, close them ([`Action::UnlockAll`],
//! [`Action::Open`], [`Action::OpenWithKeyFile`], [`Action::Close`]), as
//! `zluks` and start-up need them. Each runs as one unlocking run of
//! `zbm-luks`, which drops its secrets at the end.

use std::collections::BTreeSet;
use std::path::Path;

use zbm_config::spec::cmdline::luks::LUKS_KEY;
use zbm_crypt::OpenOptions;
use zbm_luks::LuksError;
use zbm_luks::device::resolve_device;
use zbm_luks::mappings::{Mapping, Origin};
use zbm_luks::unlock::methods::{Refusal, Registry};
use zbm_luks::unlocker::{Luks, Unlocker};
use zbm_types::{DevicePath, MappingName};

use crate::model::{Action, Event, Outcome, UnlockOutcome};

/// Why a LUKS action fails at once in an image without `cryptsetup`.
pub const NO_CRYPTSETUP: &str = "cryptsetup is not available in this image";

/// Why [`Action::Open`] fails when the device `spec` names is not there.
pub fn device_not_found(spec: &str) -> String {
    format!("device '{spec}' not found")
}

/// The reasons an error gives, outermost first.
pub(crate) fn causes(error: &dyn std::error::Error) -> Vec<String> {
    let mut why = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        why.push(cause.to_string());
        source = cause.source();
    }
    why
}

/// The failure of `what`, for the reasons given.
fn failed(what: &str, why: Vec<String>) -> Event {
    Event::Finished(Outcome::Failed {
        what: what.to_owned(),
        why,
        next: Vec::new(),
    })
}

/// The session of `luks`, unless `cryptsetup` cannot be used.
fn session(luks: Luks<'_>) -> Option<Unlocker<'_>> {
    let mut session = Unlocker::new(luks);
    session.available().then_some(session)
}

/// The failure when `cryptsetup` cannot be used.
fn unavailable() -> Vec<Event> {
    vec![failed("unable to unlock", vec![NO_CRYPTSETUP.to_owned()])]
}

/// The mappings recorded now.
fn recorded(session: &Unlocker<'_>) -> Vec<Mapping> {
    session.mappings().read().unwrap_or_default()
}

/// The devices recorded as skipped now. Reading the state files runs no
/// program, so the runs do what `zluks` did before them, command for command.
fn skipped(session: &Unlocker<'_>) -> BTreeSet<String> {
    let path = &session.settings().skipped;
    session
        .context()
        .files()
        .read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Performs a volume action with `luks`; `None` for other actions.
pub(crate) fn perform(luks: Luks<'_>, action: &Action) -> Option<Vec<Event>> {
    Some(match action {
        Action::UnlockAll => unlock_all(luks),
        Action::Open { device, name } => open(luks, device, name.as_ref()),
        Action::OpenWithKeyFile {
            device,
            name,
            key_file,
            options,
        } => open_with_key_file(luks, (device, name), key_file, *options),
        Action::Close(only) => close(luks, only.as_ref()),
        _ => return None,
    })
}

/// Unlocks every volume; an event for each volume opened or skipped in the
/// run, then how the run ended.
fn unlock_all(luks: Luks<'_>) -> Vec<Event> {
    let Some(mut session) = session(luks) else {
        return unavailable();
    };
    let before: BTreeSet<String> = recorded(&session)
        .into_iter()
        .map(|mapping| mapping.name)
        .collect();
    let skipped_before = skipped(&session);
    let result = session.unlock_all();
    let mut events: Vec<Event> = recorded(&session)
        .into_iter()
        .filter(|mapping| !before.contains(&mapping.name))
        .map(|mapping| Event::UnlockFinished {
            volume: mapping.name,
            outcome: UnlockOutcome::Opened,
        })
        .collect();
    events.extend(skipped(&session).difference(&skipped_before).map(|device| {
        Event::UnlockFinished {
            volume: device.clone(),
            outcome: UnlockOutcome::Skipped,
        }
    }));
    events.push(match result {
        Ok(summary) => Event::Finished(Outcome::Done(format!(
            "unlocked {} LUKS volumes, {} not opened",
            summary.unlocked, summary.failed
        ))),
        Err(error) => failed("unable to unlock every LUKS volume", causes(&error)),
    });
    events
}

/// Opens the volume `spec` as `name`, with the key of `zbm.luks_key`.
fn open(luks: Luks<'_>, spec: &str, name: Option<&MappingName>) -> Vec<Event> {
    let Some(mut session) = session(luks) else {
        return unavailable();
    };
    let context = *session.context();
    let Some(device) = resolve_device(&context, spec) else {
        return vec![failed(
            &format!("unable to open {spec}"),
            vec![device_not_found(spec)],
        )];
    };
    let key = session.option(&LUKS_KEY).unwrap_or_default();
    let opened = session.open_device(name, &device, &key, "", Origin::Configured);
    session.forget_secrets();
    session.settle();
    // The name the run recorded, without asking cryptsetup again.
    let volume = match name {
        Some(name) => name.to_string(),
        None => recorded(&session)
            .into_iter()
            .rev()
            .find(|mapping| mapping.device == device.as_str())
            .map_or_else(|| device.to_string(), |mapping| mapping.name),
    };
    match opened {
        Ok(_) => vec![
            Event::UnlockFinished {
                volume: volume.clone(),
                outcome: UnlockOutcome::Opened,
            },
            Event::Finished(Outcome::Done(format!("{device} is open as {volume}"))),
        ],
        Err(error) => {
            let why = causes(&error);
            vec![
                Event::UnlockFinished {
                    volume,
                    outcome: UnlockOutcome::Failed(why.join(": ")),
                },
                failed(&format!("unable to open {device}"), why),
            ]
        }
    }
}

/// Opens `device` as `name` with the key file `key_file` and no other key
/// method; when it does not open, every attempt is a reason.
fn open_with_key_file(
    luks: Luks<'_>,
    (device, name): (&DevicePath, &MappingName),
    key_file: &Path,
    options: OpenOptions,
) -> Vec<Event> {
    let Some(session) = session(luks) else {
        return unavailable();
    };
    let mut session = session.with_registry(Registry::key_file());
    // The crypttab options of the arguments asked for.
    let text: Vec<&str> = [
        (options.allow_discards, "discard"),
        (options.read_only, "readonly"),
    ]
    .into_iter()
    .filter_map(|(is_on, option)| is_on.then_some(option))
    .collect();
    let keyspec = key_file.to_string_lossy();
    let opened = session.open_device(
        Some(name),
        device,
        &keyspec,
        &text.join(","),
        Origin::Configured,
    );
    session.forget_secrets();
    let volume = name.to_string();
    match opened {
        Ok(_) => vec![
            Event::UnlockFinished {
                volume,
                outcome: UnlockOutcome::Opened,
            },
            Event::Finished(Outcome::Done(format!(
                "opened {device} as {} (key file)",
                name.mapper_path()
            ))),
        ],
        Err(error) => {
            let mut why = causes(&error);
            if let LuksError::NotUnlocked { attempts, .. } = &error {
                // A volume not unlocked keeps only the methods that failed.
                why.extend(attempts.iter().filter_map(|attempt| {
                    let Some(Refusal::Failed(error)) = &attempt.refusal else {
                        return None;
                    };
                    Some(format!("{:?}: {error}", attempt.method))
                }));
            }
            vec![
                Event::UnlockFinished {
                    volume,
                    outcome: UnlockOutcome::Failed(why.join(": ")),
                },
                failed(&format!("unable to open {device}"), why),
            ]
        }
    }
}

/// Closes the mappings ZFSBootMenu opened, or the one named `only`.
fn close(luks: Luks<'_>, only: Option<&MappingName>) -> Vec<Event> {
    let Some(mut session) = session(luks) else {
        return unavailable();
    };
    match session.close_all(only) {
        Ok(()) => vec![Event::Finished(Outcome::Done(match only {
            Some(name) => format!("closed {name}"),
            None => "closed the LUKS volumes".to_owned(),
        }))],
        Err(error) => vec![failed("unable to close", causes(&error))],
    }
}

#[cfg(test)]
mod tests;
