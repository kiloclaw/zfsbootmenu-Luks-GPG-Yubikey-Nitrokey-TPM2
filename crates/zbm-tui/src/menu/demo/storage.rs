//! What the demo machine does to its pools, environments and snapshots;
//! the snapshots are kept apart from the machine's snapshot ([`Lists`]).

use zbm_types::{DatasetName, PoolName, SnapshotName, Untrusted};

use super::data::NOW;
use super::snapshots::Lists;
use super::untrusted;
use crate::menu::model::own_command_line;
use crate::menu::world::{
    Access, Action, CommandLine, Environment, Event, ImportOptions, Origin, Outcome, Snapshot,
    SnapshotInfo, restore_snapshot,
};

/// Applies an action on storage, and says what happened.
pub(super) fn apply(snapshot: &mut Snapshot, lists: &mut Lists, action: Action) -> Vec<Event> {
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
        Action::BootSnapshot(name, new) => {
            return vec![Event::Leaving(format!(
                "Booting {new}, a new clone of {name}"
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
        Action::SetCommandLine(name, line) => set_command_line(snapshot, &name, Some(line)),
        Action::InheritCommandLine(name) => set_command_line(snapshot, &name, None),
        Action::CreateSnapshot(name) => create(snapshot, lists, &name),
        Action::Rollback(target) => rollback(snapshot, lists, &target),
        Action::Destroy(names) => destroy(snapshot, lists, &names),
        Action::Clone(name, new)
        | Action::Duplicate(name, new)
        | Action::CloneAndPromote(name, new) => copy(snapshot, &name, &new),
        Action::ResetBootTries(name) => reset_tries(snapshot, &name),
        Action::Export(pool) => export(snapshot, &pool),
        _ => return Vec::new(),
    };
    lists.show(snapshot);
    vec![
        Event::Changed(Box::new(snapshot.clone())),
        Event::Finished(outcome),
    ]
}

/// What the last frame says when the machine boots.
fn boot_message(
    snapshot: &Snapshot,
    name: &DatasetName,
    kernel: Option<Untrusted<String>>,
    command_line: Option<Untrusted<String>>,
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
        .or_else(|| environment.map(|e| own_command_line(e).clone()))
        .unwrap_or_default();
    format!(
        "Booting {name} with kernel {} and the command line \"{}\"",
        kernel.printable(),
        line.printable()
    )
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

fn set_kernel(
    snapshot: &mut Snapshot,
    name: &DatasetName,
    version: Option<&Untrusted<String>>,
) -> Outcome {
    let note = make_writable(snapshot, &name.pool());
    let Some(environment) = environment(snapshot, name) else {
        return Outcome::Warning(format!("{name} is gone"));
    };
    for kernel in &mut environment.kernels {
        kernel.is_default = Some(&kernel.version) == version;
    }
    let what = version.map_or_else(
        || format!("{name} boots its newest kernel"),
        |version| format!("{name} boots {} by default", version.printable()),
    );
    Outcome::Done(format!("{what}{note}"))
}

/// Saves `line` as the command line of `name`, or, without one or with an
/// empty one, lets it inherit the line of its parent: in `zroot`,
/// `zroot/ROOT` sets `%{parent} loglevel=4` and `zroot` sets `quiet`; in
/// `tank`, none is set.
fn set_command_line(
    snapshot: &mut Snapshot,
    name: &DatasetName,
    line: Option<Untrusted<String>>,
) -> Outcome {
    let note = make_writable(snapshot, &name.pool());
    let Some(environment) = environment(snapshot, name) else {
        return Outcome::Warning(format!("{name} is gone"));
    };
    if let Some(line) = line.filter(|line| !line.get().is_empty()) {
        environment.command_line = CommandLine {
            value: line.clone(),
            origin: Origin::Local,
            expanded: line,
        };
        return Outcome::Done(format!("the command line of {name} is saved{note}"));
    }
    environment.command_line = if name.pool().as_str() == "zroot" {
        let from = "zroot/ROOT".parse().ok();
        CommandLine {
            value: untrusted("%{parent} loglevel=4"),
            origin: from.map_or(Origin::Default, Origin::Inherited),
            expanded: untrusted("quiet loglevel=4"),
        }
    } else {
        CommandLine {
            value: Untrusted::default(),
            origin: Origin::Default,
            expanded: Untrusted::default(),
        }
    };
    Outcome::Done(format!("{name} inherits its command line{note}"))
}

/// Takes `snapshot_name`; one that exists fails as `zfs snapshot` does,
/// with its message.
fn create(snapshot: &mut Snapshot, lists: &mut Lists, snapshot_name: &SnapshotName) -> Outcome {
    let name = snapshot_name.dataset();
    let note = make_writable(snapshot, &name.pool());
    if environment(snapshot, name).is_none() {
        return Outcome::Warning(format!("{name} is gone"));
    }
    let own = lists.of_mut(name);
    if own.iter().any(|s| &s.name == snapshot_name) {
        return Outcome::Failed {
            what: format!("unable to take {snapshot_name}"),
            why: vec![format!(
                "cannot create snapshot '{snapshot_name}': dataset already exists"
            )],
            next: Vec::new(),
        };
    }
    own.insert(
        0,
        SnapshotInfo {
            name: snapshot_name.clone(),
            created: NOW,
            used: 0,
        },
    );
    Outcome::Done(format!("{snapshot_name} created{note}"))
}

fn rollback(snapshot: &mut Snapshot, lists: &mut Lists, target: &SnapshotName) -> Outcome {
    let name = target.dataset().clone();
    if environment(snapshot, &name).is_none() {
        return Outcome::Warning(format!("{name} is gone"));
    }
    let own = lists.of_mut(&name);
    let Some(created) = own.iter().find(|s| &s.name == target).map(|s| s.created) else {
        return Outcome::Warning(format!("{target} is gone"));
    };
    let before = own.len();
    own.retain(|s| s.created <= created);
    let destroyed = before - own.len();
    Outcome::Done(format!(
        "{name} rolled back to {}; {destroyed} newer snapshots destroyed",
        target.name()
    ))
}

/// Destroys `names`, of one environment, all of them or none: a clone or
/// a hold of one stops it, as `zfs destroy` refuses it.
fn destroy(snapshot: &mut Snapshot, lists: &mut Lists, names: &[SnapshotName]) -> Outcome {
    let Some(first) = names.first() else {
        return Outcome::Warning("no snapshot to destroy".to_owned());
    };
    let environment = first.dataset().clone();
    let stops: Vec<String> = names
        .iter()
        .filter_map(super::review::stop)
        .map(|stop| super::review::why(&stop))
        .collect();
    if !stops.is_empty() {
        // As `zbm-app` says it: `a snapshot`, `3 snapshots`.
        let counted = match names.len() {
            1 => "a snapshot".to_owned(),
            count => format!("{count} snapshots"),
        };
        return Outcome::Failed {
            what: format!("unable to destroy {counted} of {environment}"),
            why: stops,
            next: Vec::new(),
        };
    }
    let note = make_writable(snapshot, &environment.pool());
    lists
        .of_mut(&environment)
        .retain(|s| !names.contains(&s.name));
    let short: Vec<&str> = names.iter().map(SnapshotName::name).collect();
    Outcome::Done(format!("{environment}@{} destroyed{note}", short.join(",")))
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
        snapshot_count: 0,
        newest_snapshot: None,
        tries: None,
        ..source
    });
    Outcome::Done(format!("{new} created from {}{}", name.name(), note))
}

/// Restores `count` paths from `from` into its environment: the pool is
/// imported read/write and a safety snapshot taken first.
pub(super) fn restore(
    snapshot: &mut Snapshot,
    lists: &mut Lists,
    from: &SnapshotName,
    count: usize,
) -> Outcome {
    let environment = from.dataset();
    let note = make_writable(snapshot, &environment.pool());
    let Some(safety) = restore_snapshot(environment, snapshot.now) else {
        return Outcome::Warning(format!("no safety snapshot for {environment}"));
    };
    let files = if count == 1 { "file" } else { "files" };
    let outcome = create(snapshot, lists, &safety);
    lists.show(snapshot);
    match outcome {
        Outcome::Done(_) => Outcome::Done(format!(
            "{count} {files} of {environment} restored from {}{note}; @{} undoes it",
            from.name(),
            safety.name()
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

/// An import of `pool`, which is imported: read-only or read/write
/// again, or rewound to its checkpoint.
pub(super) fn reimport(
    snapshot: &mut Snapshot,
    pool: &PoolName,
    options: ImportOptions,
) -> Outcome {
    if options.rewind {
        return rewind(snapshot, pool);
    }
    match options.access {
        Access::ReadWrite if make_writable(snapshot, pool).is_empty() => {
            Outcome::Done(format!("{pool} was imported read/write already"))
        }
        Access::ReadWrite => Outcome::Done(format!("{pool} imported read/write")),
        Access::ReadOnly => {
            if let Some(found) = snapshot.pools.iter_mut().find(|p| &p.name == pool) {
                found.writable = false;
            }
            Outcome::Done(format!("{pool} imported read-only"))
        }
    }
}

fn rewind(snapshot: &mut Snapshot, pool: &PoolName) -> Outcome {
    match snapshot.pools.iter_mut().find(|p| &p.name == pool) {
        Some(found) if found.checkpoint == Some(true) => {
            found.checkpoint = Some(false);
            found.writable = true;
            Outcome::Done(format!("{pool} rewound to its checkpoint"))
        }
        _ => Outcome::Warning(format!("{pool} has no checkpoint")),
    }
}

/// Exports `pool`, which a scan finds then: its environments go, and the
/// default with them when it was one of them.
fn export(snapshot: &mut Snapshot, pool: &PoolName) -> Outcome {
    let Some(found) = snapshot
        .pools
        .iter_mut()
        .find(|known| known.imported && &known.name == pool)
    else {
        return Outcome::Warning(format!("{pool} is not imported"));
    };
    found.imported = false;
    found.writable = false;
    snapshot
        .environments
        .retain(|environment| &environment.name.pool() != pool);
    if snapshot
        .bootfs
        .as_ref()
        .is_some_and(|bootfs| &bootfs.pool() == pool)
    {
        snapshot.bootfs = None;
    }
    Outcome::Done(format!("{pool} is exported"))
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
        snapshot_count: 0,
        newest_snapshot: None,
        tries: None,
        command_line: CommandLine {
            value: untrusted("quiet"),
            origin: Origin::Local,
            expanded: untrusted("quiet"),
        },
        ..template
    });
}
