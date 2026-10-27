//! The [`Snapshot`] of the machine, read from it: its pools, boot
//! environments, LUKS volumes, the countdown, the keyboard layouts and the
//! log (R4 of ROADMAP.md).
//!
//! What the runtime cannot read yet stays empty: the kernels of a boot
//! environment, its checks and boot tries arrive with R5 and port package
//! C; the stages of start-up arrive as events (R4.2). The snapshots of
//! each environment are read with it, two listings each; the performance
//! test of R5 step 7 decides whether they load on demand instead.

use std::collections::BTreeSet;
use std::path::Path;

use zbm_config::options::ZbmOptions;
use zbm_luks::device::is_open;
use zbm_luks::tokens::list_tokens;
use zbm_luks::unlocker::Unlocker;
use zbm_platform::Files;
use zbm_state::State;
use zbm_types::{DatasetName, PoolName};
use zbm_zfs::{Source, Zfs, ZfsError};

use crate::Encryption;
use crate::model::{
    CommandLine, Environment, Health, Keyboard, Level, Log, LogEntry, Method, Origin, Pool,
    SnapshotInfo, Timeout, Volume, VolumeState,
};

/// The property that holds a boot environment's command line.
const COMMAND_LINE: &str = "org.zfsbootmenu:commandline";

/// The properties a boot environment is told apart by.
const PROPERTIES: &[&str] = &["mountpoint", "canmount", "keystatus"];

/// The index of the image's keyboard layouts, in their directory.
const LAYOUT_INDEX: &str = "layouts";

/// The health of a pool, in the four the menu tells apart.
fn health(health: &zbm_zfs::Health) -> Health {
    match health {
        zbm_zfs::Health::Online => Health::Online,
        zbm_zfs::Health::Degraded => Health::Degraded,
        zbm_zfs::Health::Faulted | zbm_zfs::Health::Suspended => Health::Faulted,
        _ => Health::Unavailable,
    }
}

/// The imported pools.
///
/// # Errors
///
/// When `zpool list` fails or prints something unexpected.
pub fn pools(zfs: &dyn Zfs) -> Result<Vec<Pool>, ZfsError> {
    Ok(zfs
        .pools()?
        .into_iter()
        .map(|pool| {
            let checkpoint = zfs
                .pool_property(&pool.name, "checkpoint")
                .is_ok_and(|property| property.value().is_some());
            Pool {
                health: health(&pool.health),
                writable: !pool.read_only,
                imported: true,
                foreign_hostid: None,
                checkpoint,
                name: pool.name,
            }
        })
        .collect())
}

/// Where a property was set, as the menu says it.
fn origin(source: &Source) -> Origin {
    match source {
        Source::Local | Source::Received | Source::Temporary => Origin::Local,
        Source::Inherited(from) => from
            .parse::<DatasetName>()
            .map_or(Origin::Default, Origin::Inherited),
        Source::Default | Source::None => Origin::Default,
    }
}

/// The command line of `dataset`, with where it was set.
fn command_line(zfs: &dyn Zfs, dataset: &DatasetName) -> CommandLine {
    match zfs.property(dataset, COMMAND_LINE) {
        Ok(property) => CommandLine {
            value: property.value().unwrap_or_default().to_owned(),
            origin: if property.value().is_some() {
                origin(&property.source)
            } else {
                Origin::Default
            },
        },
        Err(_) => CommandLine {
            value: String::new(),
            origin: Origin::Default,
        },
    }
}

/// The snapshots of `environment` itself, newest first, with the space
/// each holds; none when they cannot be listed.
fn snapshots(zfs: &dyn Zfs, environment: &DatasetName) -> Vec<SnapshotInfo> {
    let listed = zfs.snapshots(environment).unwrap_or_default();
    let space: std::collections::BTreeMap<_, _> = zfs
        .snapshot_space(environment)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mut found: Vec<SnapshotInfo> = listed
        .into_iter()
        .filter(|snapshot| snapshot.name.dataset() == environment)
        .map(|snapshot| SnapshotInfo {
            used: space.get(&snapshot.name).copied().unwrap_or_default(),
            created: snapshot.creation,
            name: snapshot.name,
        })
        .collect();
    found.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.created));
    found
}

/// The boot environments: filesystems mounted at `/` that can be mounted,
/// each with whether it is its pool's `bootfs`, its encryption and its
/// command line.
///
/// # Errors
///
/// When `zfs list` or `zpool list` fails or prints something unexpected.
pub fn environments(zfs: &dyn Zfs) -> Result<Vec<Environment>, ZfsError> {
    let defaults: BTreeSet<DatasetName> = zfs
        .pools()?
        .into_iter()
        .filter_map(|pool| pool.bootfs)
        .collect();
    Ok(zfs
        .filesystems(None, PROPERTIES)?
        .into_iter()
        .filter(|dataset| {
            dataset.value("mountpoint") == Some("/") && dataset.value("canmount") != Some("off")
        })
        .map(|dataset| {
            let encryption = match dataset.value("keystatus") {
                None => Encryption::None,
                Some("available") => Encryption::Unlocked,
                Some(_) => Encryption::Locked,
            };
            Environment {
                is_default: defaults.contains(&dataset.name),
                encryption,
                kernels: Vec::new(),
                command_line: command_line(zfs, &dataset.name),
                snapshots: snapshots(zfs, &dataset.name),
                checks: Vec::new(),
                tries: None,
                name: dataset.name,
            }
        })
        .collect())
}

/// The key method of a LUKS2 token type, if the menu shows one for it.
fn token_method(kind: &str, has_pin: bool) -> Option<Method> {
    match kind {
        "systemd-tpm2" if has_pin => Some(Method::Tpm2Pin),
        "systemd-tpm2" => Some(Method::Tpm2),
        "systemd-fido2" => Some(Method::Fido2),
        "clevis" => Some(Method::Clevis),
        _ => None,
    }
}

/// The LUKS volumes the session finds: each with its mapping when
/// ZFSBootMenu opened it, whether it is open, the methods its tokens offer
/// (the passphrase last) and the pools on it.
pub fn volumes(session: &Unlocker<'_>) -> Vec<Volume> {
    let system = *session.system();
    let settings = session.settings();
    let mappings = session.mappings().read().unwrap_or_default();
    session
        .discover_devices()
        .into_iter()
        .map(|device| {
            let mapping = mappings.iter().find(|mapping| mapping.device == device);
            let open = is_open(system.files, &device, &settings.open_check);
            let mut methods: Vec<Method> = Vec::new();
            for token in list_tokens(&system, &device) {
                let has_pin = token.flag("tpm2-pin").unwrap_or(false);
                if let Some(method) = token_method(&token.kind, has_pin)
                    && !methods.contains(&method)
                {
                    methods.push(method);
                }
            }
            methods.push(Method::Passphrase);
            let pool = mapping
                .map(|mapping| settings.mapper.join(&mapping.name))
                .and_then(|path| {
                    session
                        .pools_on_device(&path.to_string_lossy())
                        .into_iter()
                        .find_map(|pool| pool.parse::<PoolName>().ok())
                });
            Volume {
                name: mapping.map_or_else(|| device.clone(), |mapping| mapping.name.clone()),
                state: if open {
                    VolumeState::Open
                } else {
                    VolumeState::Locked
                },
                methods,
                pool,
                tpm2: None,
                device,
            }
        })
        .collect()
}

/// What a stage of start-up does, for people.
pub fn stage_name(stage: zbm_types::Stage) -> &'static str {
    match stage {
        zbm_types::Stage::Modules => "Loading kernel modules",
        zbm_types::Stage::HostId => "Setting the host ID",
        zbm_types::Stage::DeviceWait => "Waiting for devices",
        zbm_types::Stage::EarlyHooks => "Running the early setup hooks",
        zbm_types::Stage::LuksUnlock => "Unlocking LUKS volumes",
        zbm_types::Stage::ImportPools => "Importing pools",
    }
}

/// The stages of start-up: done when the state directory records them
/// finished, else pending.
pub fn startup(state: &State<'_>) -> Vec<crate::model::Stage> {
    let finished = state.finished_stages();
    zbm_types::Stage::ALL
        .iter()
        .map(|stage| crate::model::Stage {
            name: stage_name(*stage).to_owned(),
            state: if finished.iter().any(|done| done == stage.as_str()) {
                crate::model::StageState::Done
            } else {
                crate::model::StageState::Pending
            },
        })
        .collect()
}

/// The countdown `zbm.timeout` asks for: no number or a negative one waits,
/// zero boots at once.
pub fn timeout(options: &ZbmOptions) -> Timeout {
    match options.menu_timeout_seconds() {
        Some(0) => Timeout::Skip,
        Some(seconds) => u32::try_from(seconds).map_or(Timeout::Wait, Timeout::Seconds),
        None => Timeout::Wait,
    }
}

/// The keyboard: the image's layouts in `directory`, the one the console
/// types with first, and whether keys come from the machine's keyboard
/// rather than a serial terminal (`console` names the control terminal).
pub fn keyboard(files: &dyn Files, directory: &Path, state: &State<'_>, console: &str) -> Keyboard {
    let index = files
        .read_to_string(&directory.join(LAYOUT_INDEX))
        .unwrap_or_default();
    let mut layouts: Vec<String> = index
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    if let Some(current) = state.console_layout() {
        layouts.retain(|name| *name != current);
        layouts.insert(0, current);
    }
    let serial = ["ttyS", "ttyAMA", "ttyUSB", "hvc"]
        .iter()
        .any(|prefix| console.trim_start_matches("/dev/").starts_with(prefix));
    Keyboard {
        layouts,
        local: !serial,
    }
}

/// The warnings and errors of the runtime journal (`journal.jsonl` in the
/// state directory `base`); the rest of the journal is loaded on demand.
pub fn log(files: &dyn Files, base: &Path) -> Log {
    let text = files
        .read_to_string(&base.join("journal.jsonl"))
        .unwrap_or_default();
    let entries = text
        .lines()
        .filter_map(|line| {
            let record: serde_json::Value = serde_json::from_str(line).ok()?;
            let level = match record.get("level")?.as_str()? {
                "WARN" => Level::Warning,
                "ERROR" => Level::Error,
                _ => return None,
            };
            let text = record
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            Some(LogEntry { level, text })
        })
        .collect();
    Log { entries }
}

#[cfg(test)]
pub(crate) mod tests;
