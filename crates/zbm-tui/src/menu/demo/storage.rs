//! What the demo machine does to its pools, environments and snapshots.

use zbm_types::{DatasetName, PoolName, SnapshotName};

use super::data::NOW;
use crate::menu::world::{
    Action, CommandLine, Environment, Event, Origin, Outcome, RESTORE_SNAPSHOT, Snapshot,
    SnapshotInfo,
};

/// Applies an action on storage, and says what happened.
pub(super) fn apply(snapshot: &mut Snapshot, action: Action) -> Vec<Event> {
    let outcome = match action {
        Action::Boot {
            environment,
            kernel,
            command_line,
            ..
        } => {
            return vec![Event::Leaving(boot_message(
                snapshot,
                &environment,
                kernel,
                command_line,
            ))];
        }
        Action::BootSnapshot(name) => {
            return vec![Event::Leaving(format!(
                "Booting a temporary clone of {name}"
            ))];
        }
        Action::Chroot(target) => {
            return vec![Event::Leaving(format!(
                "Opening a shell in a chroot of {target}"
            ))];
        }
        Action::SetDefaultEnvironment(name) => set_default(snapshot, &name),
        Action::SetDefaultKernel(name, version) => set_kernel(snapshot, &name, Some(&version)),
        Action::InheritDefaultKernel(name) => set_kernel(snapshot, &name, None),
        Action::CreateSnapshot(name) => create(snapshot, &name),
        Action::Rollback(target) => rollback(snapshot, &target),
        Action::Clone(name, new)
        | Action::Duplicate(name, new)
        | Action::CloneAndPromote(name, new) => copy(snapshot, &name, &new),
        Action::ResetBootTries(name) => reset_tries(snapshot, &name),
        Action::ImportReadWrite(pool) => writable(snapshot, &pool),
        Action::RewindCheckpoint(pool) => rewind(snapshot, &pool),
        _ => return Vec::new(),
    };
    vec![Event::Changed(snapshot.clone()), Event::Finished(outcome)]
}

/// What the last frame says when the machine boots.
fn boot_message(
    snapshot: &Snapshot,
    name: &DatasetName,
    kernel: Option<String>,
    command_line: Option<String>,
) -> String {
    let environment = snapshot.environments.iter().find(|e| &e.name == name);
    let kernel = kernel
        .or_else(|| {
            environment
                .and_then(|e| e.boot_kernel())
                .map(|k| k.version.clone())
        })
        .unwrap_or_default();
    let line = command_line
        .or_else(|| environment.map(|e| e.command_line.value.clone()))
        .unwrap_or_default();
    format!("Booting {name} with kernel {kernel} and the command line \"{line}\"")
}

/// Imports `pool` read/write, as every change needs; returns a note to add
/// to the outcome when it was read-only, else nothing.
fn make_writable(snapshot: &mut Snapshot, pool: &PoolName) -> String {
    match snapshot.pools.iter_mut().find(|p| &p.name == pool) {
        Some(found) if !found.writable => {
            found.writable = true;
            format!(" ({pool} imported read/write)")
        }
        _ => String::new(),
    }
}

fn environment<'a>(snapshot: &'a mut Snapshot, name: &DatasetName) -> Option<&'a mut Environment> {
    snapshot.environments.iter_mut().find(|e| &e.name == name)
}

fn set_default(snapshot: &mut Snapshot, name: &DatasetName) -> Outcome {
    let pool = name.pool();
    let note = make_writable(snapshot, &pool);
    for environment in &mut snapshot.environments {
        if environment.name.pool() == pool {
            environment.is_default = &environment.name == name;
        }
    }
    Outcome::Done(format!("{name} is the default of {pool}{note}"))
}

fn set_kernel(snapshot: &mut Snapshot, name: &DatasetName, version: Option<&str>) -> Outcome {
    let note = make_writable(snapshot, &name.pool());
    let Some(environment) = environment(snapshot, name) else {
        return Outcome::Warning(format!("{name} is gone"));
    };
    for kernel in &mut environment.kernels {
        kernel.is_default = Some(kernel.version.as_str()) == version;
    }
    let what = version.map_or_else(
        || format!("{name} boots its newest kernel"),
        |version| format!("{name} boots {version} by default"),
    );
    Outcome::Done(format!("{what}{note}"))
}

fn create(snapshot: &mut Snapshot, snapshot_name: &SnapshotName) -> Outcome {
    let name = snapshot_name.dataset();
    let note = make_writable(snapshot, &name.pool());
    let Some(environment) = environment(snapshot, name) else {
        return Outcome::Warning(format!("{name} is gone"));
    };
    if environment
        .snapshots
        .iter()
        .any(|s| &s.name == snapshot_name)
    {
        return Outcome::Warning(format!("{snapshot_name} exists"));
    }
    environment.snapshots.insert(
        0,
        SnapshotInfo {
            name: snapshot_name.clone(),
            created: NOW,
            used: 0,
        },
    );
    Outcome::Done(format!("{snapshot_name} created{note}"))
}

fn rollback(snapshot: &mut Snapshot, target: &SnapshotName) -> Outcome {
    let name = target.dataset().clone();
    let Some(environment) = environment(snapshot, &name) else {
        return Outcome::Warning(format!("{name} is gone"));
    };
    let Some(created) = environment
        .snapshots
        .iter()
        .find(|s| &s.name == target)
        .map(|s| s.created)
    else {
        return Outcome::Warning(format!("{target} is gone"));
    };
    let before = environment.snapshots.len();
    environment.snapshots.retain(|s| s.created <= created);
    let destroyed = before - environment.snapshots.len();
    Outcome::Done(format!(
        "{name} rolled back to {}; {destroyed} newer snapshots destroyed",
        target.name()
    ))
}

fn copy(snapshot: &mut Snapshot, name: &SnapshotName, new: &DatasetName) -> Outcome {
    let pool = name.pool();
    let note = make_writable(snapshot, &pool);
    let Some(source) = snapshot
        .environments
        .iter()
        .find(|e| &e.name == name.dataset())
        .cloned()
    else {
        return Outcome::Warning(format!("{} is gone", name.dataset()));
    };
    if snapshot.environments.iter().any(|e| &e.name == new) {
        return Outcome::Warning(format!("{new} exists"));
    }
    let command_line = CommandLine {
        origin: Origin::Inherited(source.name.clone()),
        ..source.command_line
    };
    snapshot.environments.push(Environment {
        name: new.clone(),
        is_default: false,
        command_line,
        snapshots: Vec::new(),
        tries: None,
        ..source
    });
    Outcome::Done(format!("{new} created from {}{}", name.name(), note))
}

/// Restores `count` paths from `from` into its environment: the pool is
/// imported read/write and a safety snapshot taken first.
pub(super) fn restore(snapshot: &mut Snapshot, from: &SnapshotName, count: usize) -> Outcome {
    let environment = from.dataset();
    let note = make_writable(snapshot, &environment.pool());
    let Ok(safety) = format!("{environment}@{RESTORE_SNAPSHOT}").parse::<SnapshotName>() else {
        return Outcome::Warning(format!("no safety snapshot for {environment}"));
    };
    let safety = create(snapshot, &safety);
    let files = if count == 1 { "file" } else { "files" };
    match safety {
        Outcome::Done(_) => Outcome::Done(format!(
            "{count} {files} of {environment} restored from {}{note}; @before-restore undoes it",
            from.name()
        )),
        other => other,
    }
}

/// Gives `name` its boot tries back, and ends the fallback it caused.
fn reset_tries(snapshot: &mut Snapshot, name: &DatasetName) -> Outcome {
    if snapshot
        .fallback
        .as_ref()
        .is_some_and(|fallback| &fallback.failed == name)
    {
        snapshot.fallback = None;
    }
    match environment(snapshot, name).and_then(|e| e.tries.as_mut()) {
        Some(tries) => {
            tries.left = tries.total;
            Outcome::Done(format!("{name} has its {} boot tries back", tries.total))
        }
        None => Outcome::Warning(format!("{name} does not count its boots")),
    }
}

fn writable(snapshot: &mut Snapshot, pool: &PoolName) -> Outcome {
    if make_writable(snapshot, pool).is_empty() {
        Outcome::Done(format!("{pool} was imported read/write already"))
    } else {
        Outcome::Done(format!("{pool} imported read/write"))
    }
}

fn rewind(snapshot: &mut Snapshot, pool: &PoolName) -> Outcome {
    match snapshot.pools.iter_mut().find(|p| &p.name == pool) {
        Some(found) if found.checkpoint => {
            found.checkpoint = false;
            Outcome::Done(format!("{pool} rewound to its checkpoint"))
        }
        _ => Outcome::Warning(format!("{pool} has no checkpoint")),
    }
}

/// The boot environment of the foreign pool, once it is imported.
pub(super) fn add_backup_environment(snapshot: &mut Snapshot, pool: &PoolName) {
    let Ok(name) = format!("{pool}/ROOT/void-old").parse::<DatasetName>() else {
        return;
    };
    if snapshot.environments.iter().any(|e| e.name == name) {
        return;
    }
    let Some(template) = snapshot.environments.first().cloned() else {
        return;
    };
    snapshot.environments.push(Environment {
        name,
        is_default: true,
        snapshots: Vec::new(),
        tries: None,
        command_line: CommandLine {
            value: "quiet".to_owned(),
            origin: Origin::Local,
        },
        ..template
    });
}
