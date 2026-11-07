//! The plans of the demo machine's changes, with their reviews (ADR 0016,
//! points 12 to 15), as `zbm-app` reads them from the pools: a rollback
//! loses the newer snapshots of its environment and what was written
//! since, and a clone of one, or a hold, stops it; a rewind loses what was
//! written since the checkpoint; a forced import says who used the pool
//! last. Of the default environment's daily snapshots, that of 18 October
//! has a clone and that of 19 October a hold.

use zbm_types::{ImportPolicy, PoolName, SnapshotName};

use super::data::NOW;
use super::snapshots::Lists;
use super::untrusted;
use crate::menu::world::{
    Access, Action, Event, ImportOptions, Keep, Loss, Outcome, Plan, PoolUse, Review, Safer,
    Snapshot, Step, StepKind, Stop,
};

/// The environment whose snapshots have a clone and a hold.
const ENVIRONMENT: &str = "zroot/ROOT/void";

/// The snapshot of [`ENVIRONMENT`] with a clone, and the clone.
const CLONED: (&str, &str) = ("autosnap_2026-10-18_daily", "zroot/ROOT/void-test");

/// The snapshot of [`ENVIRONMENT`] with a hold, and its tag.
const HELD: (&str, &str) = ("autosnap_2026-10-19_daily", "backup-keep");

/// What was written to an environment since the snapshot it rolls back
/// to, in bytes.
const WRITTEN: u64 = 241_172_480;

/// When the checkpoint of a pool was taken: 15 September, and as `zpool`
/// prints it.
const CHECKPOINT: (u64, &str) = (NOW - 38 * 86_400, "Tue Sep 15 08:00:00 2026");

/// The space the checkpoint holds, in bytes.
const CHECKPOINT_SPACE: u64 = 3_328_180_224;

/// When a pool of another host was last written: two days ago.
const FOREIGN_WRITTEN: u64 = NOW - 2 * 86_400 - 5 * 3600;

/// [`Action::Plan`] of `action` on the machine `snapshot`, whose snapshots
/// `lists` holds: its plan, or, for an action that has none, a failure.
pub(super) fn planned(snapshot: &Snapshot, lists: &Lists, action: Action) -> Event {
    let plan = match &action {
        Action::Rollback(target) => rollback(lists, target),
        Action::Import(pool, options) if options.rewind => rewind(snapshot, lists, pool, *options),
        Action::Import(pool, options) if options.policy == ImportPolicy::Force => {
            forced(snapshot, pool, *options)
        }
        _ => {
            return Event::Finished(Outcome::Failed {
                what: "nothing to plan".to_owned(),
                why: vec!["only a change has a plan".to_owned()],
                next: Vec::new(),
            });
        }
    };
    Event::Planned {
        action: Box::new(action),
        plan,
    }
}

/// A plan that fails for `why`, as `zfs` would.
fn failed(what: String, why: String) -> Plan {
    Plan {
        steps: Vec::new(),
        outcome: Outcome::Failed {
            what,
            why: vec![why],
            next: Vec::new(),
        },
        review: Review::default(),
    }
}

/// What stops the destroy of `snapshot`: its clone or its hold.
fn stop(snapshot: &SnapshotName) -> Option<Stop> {
    if snapshot.dataset().as_str() != ENVIRONMENT {
        return None;
    }
    match snapshot.name() {
        name if name == CLONED.0 => Some(Stop::Clone {
            snapshot: snapshot.clone(),
            clone: untrusted(CLONED.1),
        }),
        name if name == HELD.0 => Some(Stop::Hold {
            snapshot: snapshot.clone(),
            tag: Some(untrusted(HELD.1)),
        }),
        _ => None,
    }
}

/// The rollback to `target`: it loses the newer snapshots, oldest first,
/// and what was written since; it keeps the older ones; cloning `target`
/// is safer; a clone or a hold of a newer snapshot stops it.
fn rollback(lists: &Lists, target: &SnapshotName) -> Plan {
    let own = lists.of(target.dataset());
    let Some(created) = own.iter().find(|s| &s.name == target).map(|s| s.created) else {
        let what = format!("unable to roll back to {target}");
        return failed(what, format!("{target} does not exist"));
    };
    let mut newer: Vec<_> = own.iter().filter(|s| s.created > created).collect();
    newer.sort_by_key(|s| s.created);
    let stops: Vec<Stop> = newer.iter().filter_map(|s| stop(&s.name)).collect();
    let mut loses: Vec<Loss> = newer
        .iter()
        .map(|s| Loss::Snapshot {
            name: s.name.clone(),
            used: Some(s.used),
        })
        .collect();
    loses.push(Loss::Written {
        since: target.clone(),
        bytes: Some(WRITTEN),
    });
    let older = own.iter().filter(|s| s.created < created).count();
    let has_clone = stops.iter().any(|stop| matches!(stop, Stop::Clone { .. }));
    let review = Review {
        loses,
        freed: (!has_clone).then(|| newer.iter().map(|s| s.used).sum()),
        keeps: (older > 0)
            .then_some(Keep::Snapshots(older))
            .into_iter()
            .collect(),
        undo: None,
        safer: Some(Safer::Clone(target.clone())),
        stops,
        pool_use: None,
    };
    let change = Step {
        kind: StepKind::Rollback(target.clone()),
        command: Some(format!("zfs rollback -r {target}")),
    };
    plan(vec![change], review)
}

/// The plan of `steps` with `review`: done, unless something stops it.
fn plan(steps: Vec<Step>, review: Review) -> Plan {
    let outcome = if review.stops.is_empty() {
        Outcome::Done(String::new())
    } else {
        Outcome::Failed {
            what: "ZFS would refuse it".to_owned(),
            why: review.stops.iter().map(why).collect(),
            next: Vec::new(),
        }
    };
    Plan {
        steps,
        outcome,
        review,
    }
}

/// Why `stop` stops a change, as `zbm-app` says it.
fn why(stop: &Stop) -> String {
    match stop {
        Stop::Clone { snapshot, clone } => {
            format!("{snapshot} has the clone {}", clone.printable())
        }
        Stop::Hold { snapshot, .. } => format!("{snapshot} has a hold"),
        Stop::Active { pool, .. } => format!("{pool} is imported by another system"),
    }
}

/// The import of `pool` with `options`, as its step.
fn import(pool: &PoolName, options: ImportOptions) -> Step {
    let force = if options.policy == ImportPolicy::Force {
        " -f"
    } else {
        ""
    };
    let readonly = match options.access {
        Access::ReadOnly => "on",
        Access::ReadWrite => "off",
    };
    let rewind = if options.rewind {
        " --rewind-to-checkpoint"
    } else {
        ""
    };
    Step {
        kind: StepKind::Import {
            pool: pool.clone(),
            options,
        },
        command: Some(format!(
            "zpool import -N{force} -o readonly={readonly}{rewind} {pool}"
        )),
    }
}

/// The rewind of `pool` to its checkpoint: it loses every write since, the
/// snapshots of its environments taken since among them; importing it
/// read-only without the rewind is safer; this host has it imported.
fn rewind(snapshot: &Snapshot, lists: &Lists, pool: &PoolName, options: ImportOptions) -> Plan {
    let Some(found) = snapshot
        .pools
        .iter()
        .find(|p| &p.name == pool && p.imported)
    else {
        return failed(
            format!("unable to rewind {pool}"),
            format!("{pool} is not imported"),
        );
    };
    if found.checkpoint != Some(true) {
        let why = format!("{pool} has no checkpoint");
        return failed(format!("unable to rewind {pool}"), why);
    }
    let since: Vec<SnapshotName> = snapshot
        .environments
        .iter()
        .filter(|environment| &environment.name.pool() == pool)
        .flat_map(|environment| lists.of(&environment.name))
        .filter(|s| s.created > CHECKPOINT.0)
        .map(|s| s.name.clone())
        .collect();
    let safe = ImportOptions {
        access: Access::ReadOnly,
        policy: ImportPolicy::Strict,
        rewind: false,
        id: options.id,
    };
    let review = Review {
        loses: vec![Loss::SinceCheckpoint {
            created: Some(untrusted(CHECKPOINT.1)),
            space: Some(CHECKPOINT_SPACE),
            discarding: false,
            datasets: Some(Vec::new()),
            snapshots: Some(since),
        }],
        safer: Some(Safer::Import(Box::new(Action::Import(pool.clone(), safe)))),
        pool_use: Some(PoolUse {
            this_host: true,
            ..PoolUse::default()
        }),
        ..Review::default()
    };
    let export = Step {
        kind: StepKind::Export(pool.clone()),
        command: Some(format!("zpool export {pool}")),
    };
    plan(vec![export, import(pool, options)], review)
}

/// The forced import of `pool`: an export undoes it; the host that used
/// it last, and when it was last written.
fn forced(snapshot: &Snapshot, pool: &PoolName, options: ImportOptions) -> Plan {
    let Some(found) = snapshot
        .pools
        .iter()
        .find(|p| &p.name == pool && !p.imported)
    else {
        let what = format!("unable to import {pool}");
        return failed(what, format!("{pool} is imported already"));
    };
    let review = Review {
        undo: Some(Box::new(Action::Export(pool.clone()))),
        pool_use: Some(PoolUse {
            host: found.host.clone(),
            written: Some(FOREIGN_WRITTEN),
            this_host: false,
            active: false,
        }),
        ..Review::default()
    };
    plan(vec![import(pool, options)], review)
}
