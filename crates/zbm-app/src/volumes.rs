//! The use cases of LUKS volumes: unlock every volume, open one, close
//! them ([`Action::UnlockAll`], [`Action::Open`], [`Action::Close`]), as
//! `zluks` and start-up need them. Each runs as one unlocking run of
//! `zbm-luks`, which drops its secrets at the end.

use std::collections::BTreeSet;

use zbm_config::spec::cmdline::luks::LUKS_KEY;
use zbm_luks::device::{default_name, resolve_device};
use zbm_luks::mappings::{Mapping, Origin};
use zbm_luks::unlocker::{Luks, Unlocker};

use crate::model::{Action, Event, Outcome, UnlockOutcome};

/// What `zbm-luks` says when it cannot work at all.
const NO_CRYPTSETUP: &str = "cryptsetup is not available in this image";

/// The reasons an error gives, outermost first.
fn causes(error: &dyn std::error::Error) -> Vec<String> {
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

/// Performs a volume action with `luks`; `None` for other actions.
pub(crate) fn perform(luks: Luks<'_>, action: &Action) -> Option<Vec<Event>> {
    Some(match action {
        Action::UnlockAll => unlock_all(luks),
        Action::Open { device, name } => open(luks, device, name),
        Action::Close(only) => close(luks, only.as_deref()),
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
    let devices = session.discover_devices();
    let result = session.unlock_all();
    let mut events: Vec<Event> = recorded(&session)
        .into_iter()
        .filter(|mapping| !before.contains(&mapping.name))
        .map(|mapping| Event::UnlockFinished {
            volume: mapping.name,
            outcome: UnlockOutcome::Opened,
        })
        .collect();
    let store = session.mappings();
    events.extend(
        devices
            .iter()
            .filter(|device| store.is_skipped(device))
            .map(|device| Event::UnlockFinished {
                volume: device.clone(),
                outcome: UnlockOutcome::Skipped,
            }),
    );
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
fn open(luks: Luks<'_>, spec: &str, name: &str) -> Vec<Event> {
    let Some(mut session) = session(luks) else {
        return unavailable();
    };
    let system = *session.system();
    let Some(device) = resolve_device(&system, spec) else {
        return vec![failed(
            &format!("unable to open {spec}"),
            vec![format!("device '{spec}' not found")],
        )];
    };
    let key = session.option(&LUKS_KEY).unwrap_or_default();
    let opened = session.open_device(name, &device, &key, "", Origin::Configured);
    session.forget_secrets();
    session.settle();
    let volume = if name.is_empty() {
        default_name(&system, &device)
    } else {
        name.to_owned()
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

/// Closes the mappings ZFSBootMenu opened, or the one named `only`.
fn close(luks: Luks<'_>, only: Option<&str>) -> Vec<Event> {
    let mut session = Unlocker::new(luks);
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
