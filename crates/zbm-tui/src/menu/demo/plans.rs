//! The plans of the demo machine's changes that lose nothing, as `zbm-app`
//! plans them: their steps, with the commands they would run, and what
//! undoes them. A change of a pool imported read-only imports it
//! read/write first, and one of an environment whose key is not loaded
//! loads it; a command line saved, a user property, needs no key. The
//! rollback, the destroy, the rewind and the forced import have their
//! reviews ([`super::review`]).

use std::fmt::Display;

use zbm_platform::shell::word;
use zbm_types::{DatasetName, PoolName, SnapshotName, Untrusted};

use super::review::{import, plan};
use crate::menu::world::{
    Access, Action, Encryption, ImportOptions, Origin, Plan, Review, Snapshot, Step, StepKind,
    restore_snapshot,
};

/// The property that names an environment's default kernel.
const KERNEL: &str = "org.zfsbootmenu:kernel";

/// The property that holds an environment's command line.
const COMMAND_LINE: &str = "org.zfsbootmenu:commandline";

/// The properties a clone and a copy are given.
const GIVEN: &str = "-o mountpoint=/ -o canmount=noauto";

/// The plan of `action` on the machine `snapshot`, but for those that
/// [`super::review`] plans. An action with no plan ([`Action::has_plan`]),
/// which [`super::review::planned`] refuses first, has no steps.
pub(super) fn plan_of(snapshot: &Snapshot, action: &Action) -> Plan {
    let (steps, review) = match action {
        Action::CreateSnapshot(name) => {
            let take = step(
                StepKind::Snapshot(name.clone()),
                format!("zfs snapshot {}", shell(name)),
            );
            let steps = changes(snapshot, name.dataset(), vec![take]);
            (steps, undone_by(Action::Destroy(vec![name.clone()])))
        }
        Action::Clone(from, target) => (clone(snapshot, from, target), Review::default()),
        Action::CloneAndPromote(from, target) => {
            let mut steps = clone(snapshot, from, target);
            let promote = StepKind::Promote(target.clone());
            steps.push(step(promote, format!("zfs promote {}", shell(target))));
            (steps, Review::default())
        }
        Action::BootSnapshot(from, target) => {
            let mut steps = clone(snapshot, from, target);
            steps.push(Step {
                kind: StepKind::Boot(target.clone()),
                command: None,
            });
            (steps, Review::default())
        }
        Action::Duplicate(from, target) => (duplicate(snapshot, from, target), Review::default()),
        Action::RestoreFiles {
            snapshot: from,
            paths,
        } => (restore(snapshot, from, paths), Review::default()),
        Action::SetDefaultEnvironment(environment) => default(snapshot, environment),
        Action::SetDefaultKernel(environment, version) => {
            let selector = format!("/vmlinuz-{}$", escaped(&version.printable().to_string()));
            let set = format!("{KERNEL}={selector}");
            let line = format!("zfs set {} {}", word(&set), shell(environment));
            let set = step(property(environment, false), line);
            let steps = changes(snapshot, environment, vec![set]);
            (steps, kernel_undo(snapshot, environment))
        }
        Action::InheritDefaultKernel(environment) => {
            let line = format!("zfs inherit {KERNEL} {}", shell(environment));
            let inherit = step(property(environment, true), line);
            let needs = writable(snapshot, &environment.pool());
            (
                needs.into_iter().chain([inherit]).collect(),
                Review::default(),
            )
        }
        Action::SetCommandLine(environment, line) => command_line(snapshot, environment, line),
        Action::InheritCommandLine(environment) => {
            command_line(snapshot, environment, &Untrusted::default())
        }
        Action::Import(pool, options) => import_plan(snapshot, pool, *options),
        Action::Export(pool) => (vec![export(pool)], Review::default()),
        _ => (Vec::new(), Review::default()),
    };
    plan(steps, review)
}

/// `name` as one shell word, as the machine prints its commands.
pub(super) fn shell(name: &impl Display) -> String {
    word(&name.to_string())
}

/// A step that runs `command`.
fn step(kind: StepKind, command: String) -> Step {
    Step {
        kind,
        command: Some(command),
    }
}

/// The review of a change that `undo` undoes.
fn undone_by(undo: Action) -> Review {
    Review {
        undo: Some(Box::new(undo)),
        ..Review::default()
    }
}

/// The import read/write of `pool`, when it is imported read-only.
fn writable(snapshot: &Snapshot, pool: &PoolName) -> Option<Step> {
    let read_only = snapshot
        .pools
        .iter()
        .any(|found| &found.name == pool && found.imported && !found.writable);
    read_only.then(|| Step {
        kind: StepKind::ImportReadWrite(pool.clone()),
        command: None,
    })
}

/// `steps`, a change of `environment`, after what they need: its pool
/// imported read/write, and its key loaded.
fn changes(snapshot: &Snapshot, environment: &DatasetName, steps: Vec<Step>) -> Vec<Step> {
    let locked = snapshot
        .environments
        .iter()
        .any(|found| &found.name == environment && found.encryption == Encryption::Locked);
    let key = locked.then(|| Step {
        kind: StepKind::LoadKey(environment.clone()),
        command: None,
    });
    let needs = writable(snapshot, &environment.pool())
        .into_iter()
        .chain(key);
    needs.chain(steps).collect()
}

/// The clone of `from` as `target`, after what it needs.
fn clone(snapshot: &Snapshot, from: &SnapshotName, target: &DatasetName) -> Vec<Step> {
    let kind = StepKind::Clone {
        snapshot: from.clone(),
        target: target.clone(),
    };
    let line = format!("zfs clone {GIVEN} {} {}", shell(from), shell(target));
    changes(snapshot, from.dataset(), vec![step(kind, line)])
}

/// The export of `pool`.
fn export(pool: &PoolName) -> Step {
    step(
        StepKind::Export(pool.clone()),
        format!("zpool export {}", shell(pool)),
    )
}

/// The change of the default kernel of `environment`: inherited, or set.
fn property(environment: &DatasetName, inherits: bool) -> StepKind {
    let (dataset, property) = (environment.clone(), KERNEL.to_owned());
    if inherits {
        StepKind::InheritProperty { dataset, property }
    } else {
        StepKind::SetProperty { dataset, property }
    }
}

/// The command line `line` saved for `environment`, after the read/write
/// import of its pool; inherited when it is empty. It needs no key.
fn command_line(
    snapshot: &Snapshot,
    environment: &DatasetName,
    line: &Untrusted<String>,
) -> (Vec<Step>, Review) {
    let inherits = line.get().is_empty();
    let command = if inherits {
        format!("zfs inherit {COMMAND_LINE} {}", shell(environment))
    } else {
        let set = format!("{COMMAND_LINE}={}", line.get());
        format!("zfs set {} {}", word(&set), shell(environment))
    };
    let change = step(line_property(environment, inherits), command);
    let steps = writable(snapshot, &environment.pool()).into_iter();
    (
        steps.chain([change]).collect(),
        line_undo(snapshot, environment),
    )
}

/// The change of the command line of `environment`: inherited, or set.
fn line_property(environment: &DatasetName, inherits: bool) -> StepKind {
    let (dataset, property) = (environment.clone(), COMMAND_LINE.to_owned());
    if inherits {
        StepKind::InheritProperty { dataset, property }
    } else {
        StepKind::SetProperty { dataset, property }
    }
}

/// What undoes a change of the command line of `environment`: the line
/// set on it, set again, or inheriting it again.
fn line_undo(snapshot: &Snapshot, environment: &DatasetName) -> Review {
    let found = snapshot
        .environments
        .iter()
        .find(|found| &found.name == environment);
    match found.map(|found| &found.command_line) {
        Some(line) if line.origin == Origin::Local => undone_by(Action::SetCommandLine(
            environment.clone(),
            line.value.clone(),
        )),
        _ => undone_by(Action::InheritCommandLine(environment.clone())),
    }
}

/// `text` with each character a regular expression reads escaped.
fn escaped(text: &str) -> String {
    text.chars().fold(String::new(), |mut out, c| {
        if r".+*?()[]{}|^$\".contains(c) {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

/// The copy of `from` as `target`, after what it needs.
fn duplicate(snapshot: &Snapshot, from: &SnapshotName, target: &DatasetName) -> Vec<Step> {
    let copy = StepKind::Copy {
        snapshot: from.clone(),
        target: target.clone(),
    };
    let (from_word, target_word) = (shell(from), shell(target));
    let line = format!("zfs send -p -w {from_word} | zfs recv -u {GIVEN} {target_word}");
    changes(snapshot, from.dataset(), vec![step(copy, line)])
}

/// The restore of `paths` from `from`, after its safety snapshot.
fn restore(snapshot: &Snapshot, from: &SnapshotName, paths: &[Untrusted<String>]) -> Vec<Step> {
    let environment = from.dataset();
    let safety = restore_snapshot(environment, snapshot.now).into_iter();
    let take = safety.map(|safety| {
        let line = format!("zfs snapshot {}", shell(&safety));
        step(StepKind::Snapshot(safety), line)
    });
    let restore = Step {
        kind: StepKind::Restore {
            snapshot: from.clone(),
            paths: paths.to_vec(),
        },
        command: None,
    };
    changes(snapshot, environment, take.chain([restore]).collect())
}

/// Making `environment` the default of its pool, undone by the default it
/// replaces.
fn default(snapshot: &Snapshot, environment: &DatasetName) -> (Vec<Step>, Review) {
    let pool = environment.pool();
    let kind = StepKind::SetDefault {
        pool: pool.clone(),
        environment: environment.to_string(),
    };
    let bootfs = word(&format!("bootfs={environment}"));
    let set = step(kind, format!("zpool set {bootfs} {}", shell(&pool)));
    let steps = writable(snapshot, &pool).into_iter().chain([set]).collect();
    let old = snapshot
        .environments
        .iter()
        .find(|found| found.is_default && found.name.pool() == pool && &found.name != environment);
    let review = old.map_or_else(Review::default, |old| {
        undone_by(Action::SetDefaultEnvironment(old.name.clone()))
    });
    (steps, review)
}

/// What undoes setting the default kernel of `environment`: inheriting it
/// again, when none of its kernels is its default.
fn kernel_undo(snapshot: &Snapshot, environment: &DatasetName) -> Review {
    let inherits = snapshot
        .environments
        .iter()
        .find(|found| &found.name == environment)
        .is_some_and(|found| found.kernels.iter().all(|kernel| !kernel.is_default));
    if inherits {
        undone_by(Action::InheritDefaultKernel(environment.clone()))
    } else {
        Review::default()
    }
}

/// The import of `pool` with `options`, neither forced nor rewound: one
/// imported is exported and imported with the other access, which the
/// access it has undoes; one that is not is imported read-only first,
/// adopting the host ID of its last host under that policy, which stays
/// when another system used the pool last, and is undone by its export
/// otherwise. Another pool of its name imported, which the machine
/// refuses to import, has neither steps nor undo.
fn import_plan(
    snapshot: &Snapshot,
    pool: &PoolName,
    options: ImportOptions,
) -> (Vec<Step>, Review) {
    let imported = snapshot
        .pools
        .iter()
        .find(|found| &found.name == pool && found.imported);
    if let Some(found) = imported {
        let access = if found.writable {
            Access::ReadWrite
        } else {
            Access::ReadOnly
        };
        let other = options.id.is_some_and(|id| found.id != Some(id));
        if access == options.access || other {
            return (Vec::new(), Review::default());
        }
        let undo = Action::Import(pool.clone(), ImportOptions { access, ..options });
        return (vec![export(pool), import(pool, options)], undone_by(undo));
    }
    let read_only = ImportOptions {
        access: Access::ReadOnly,
        ..options
    };
    let mut steps = vec![import(pool, read_only)];
    let foreign = snapshot.pools.iter().any(|found| {
        &found.name == pool
            && options.id.is_none_or(|id| found.id == Some(id))
            && found.host.is_some()
    });
    let review = if options.policy == zbm_types::ImportPolicy::Hostid {
        steps.push(Step {
            kind: StepKind::AdoptHostid(pool.clone()),
            command: None,
        });
        Review {
            host_id_stays: foreign,
            undo: (!foreign).then(|| Box::new(Action::Export(pool.clone()))),
            ..Review::default()
        }
    } else {
        undone_by(Action::Export(pool.clone()))
    };
    if options.access == Access::ReadWrite {
        steps.extend([export(pool), import(pool, options)]);
    }
    (steps, review)
}

#[cfg(test)]
mod tests;
