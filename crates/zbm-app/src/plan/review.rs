//! The review of a change ([`Review`]), read from the machine without
//! changing it:
//!
//! - a rollback loses the snapshots and bookmarks ZFS took after its
//!   snapshot, with their sizes and the space they free together, and
//!   what was written since; it keeps the older ones, has no undo, and
//!   cloning the snapshot is safer; a clone or a hold of a newer snapshot
//!   stops it;
//! - a rewind loses what was written since the checkpoint, among it the
//!   filesystems and snapshots made since; it has no undo, and importing
//!   the pool read-only without the rewind is safer;
//! - a forced import is undone by an export; it reads who else may use
//!   the pool, and a pool another system has imported now stops it;
//!   nothing is safer, as importing as the last host is not.
//!
//! A read that fails leaves what it gives unknown, and is logged.

use zbm_types::{DatasetName, ImportPolicy, PoolName, SnapshotName};
use zbm_zfs::{Checkpoint, SnapshotDetail, SnapshotOrBookmark, SnapshotSet, Zfs};

use crate::changes::stops;
use crate::model::{Access, Action, ImportOptions, Keep, Loss, Outcome, PoolHost, PoolUse};
use crate::model::{Review, Safer, Stop};

/// The review of `action` on the machine `zfs` reads; an empty one for an
/// action that has none.
pub(super) fn review(zfs: &dyn Zfs, action: &Action) -> Review {
    match action {
        Action::Rollback(snapshot) => rollback(zfs, snapshot),
        Action::Import(pool, options) if options.rewind => rewind(zfs, pool, *options),
        Action::Import(pool, options) if options.policy == ImportPolicy::Force => {
            forced(zfs, pool, *options)
        }
        _ => Review::default(),
    }
}

/// `outcome`, failed when `review` names what stops the change, as ZFS
/// would refuse it.
pub(super) fn refused(outcome: Outcome, review: &Review) -> Outcome {
    match outcome {
        Outcome::Done(_) if !review.stops.is_empty() => Outcome::Failed {
            what: "ZFS would refuse it".to_owned(),
            why: review.stops.iter().map(stops::why).collect(),
            next: Vec::new(),
        },
        outcome => outcome,
    }
}

fn rollback(zfs: &dyn Zfs, snapshot: &SnapshotName) -> Review {
    let mut review = Review {
        safer: Some(Safer::Clone(snapshot.clone())),
        ..Review::default()
    };
    let details = zfs
        .snapshot_details(snapshot.dataset())
        .inspect_err(|error| tracing::warn!(%error, %snapshot, "the snapshots could not be listed"))
        .unwrap_or_default();
    match stops::newer(&details, snapshot) {
        Some(newer) => {
            review.loses = newer.iter().map(|detail| lost(detail)).collect();
            review.keeps = kept(&details, snapshot);
            review.stops = stops::stops(zfs, &newer);
            review.freed = freed(zfs, &newer, &review.stops);
        }
        None => review.loses.push(Loss::Newer(snapshot.clone())),
    }
    review.loses.push(Loss::Written {
        since: snapshot.clone(),
        bytes: written(zfs, snapshot),
    });
    review
}

/// The loss of the snapshot or bookmark of `detail`.
fn lost(detail: &SnapshotDetail) -> Loss {
    match &detail.name {
        SnapshotOrBookmark::Snapshot(name) => Loss::Snapshot {
            name: name.clone(),
            used: detail.used,
        },
        SnapshotOrBookmark::Bookmark { dataset, name } => Loss::Bookmark {
            dataset: dataset.clone(),
            name: name.clone(),
        },
    }
}

/// The snapshots older than `snapshot`, and the bookmarks, among
/// `details`, that ZFS did not take after it ([`stops::newer`]).
fn kept(details: &[SnapshotDetail], snapshot: &SnapshotName) -> Vec<Keep> {
    let Some(target) = details
        .iter()
        .find(|detail| stops::is_snapshot(detail, snapshot))
    else {
        return Vec::new();
    };
    let older = details.iter().filter(|detail| {
        detail.createtxg <= target.createtxg && !stops::is_snapshot(detail, snapshot)
    });
    let (snapshots, bookmarks): (Vec<_>, Vec<_>) =
        older.partition(|detail| matches!(detail.name, SnapshotOrBookmark::Snapshot(_)));
    let mut keeps = Vec::new();
    if !snapshots.is_empty() {
        keeps.push(Keep::Snapshots(snapshots.len()));
    }
    if !bookmarks.is_empty() {
        keeps.push(Keep::Bookmarks(bookmarks.len()));
    }
    keeps
}

/// The space destroying the snapshots among `newer` frees together: none
/// without one; unknown when a clone stops the destroy, which `zfs destroy
/// -n` refuses too.
fn freed(zfs: &dyn Zfs, newer: &[&SnapshotDetail], stops: &[Stop]) -> Option<u64> {
    if stops.iter().any(|stop| matches!(stop, Stop::Clone { .. })) {
        return None;
    }
    let mut snapshots = newer.iter().filter_map(|detail| match &detail.name {
        SnapshotOrBookmark::Snapshot(name) => Some(name),
        SnapshotOrBookmark::Bookmark { .. } => None,
    });
    let Some(first) = snapshots.next() else {
        return Some(0);
    };
    let last = snapshots.next_back().unwrap_or(first);
    let range = SnapshotSet::Range {
        first: first.clone(),
        last: last.clone(),
    };
    zfs.destroy_space(&range)
        .inspect_err(|error| tracing::warn!(%error, "the space freed could not be counted"))
        .ok()
}

/// What was written to the filesystem of `snapshot` since it was taken, in
/// bytes (`written@<snapshot>`).
fn written(zfs: &dyn Zfs, snapshot: &SnapshotName) -> Option<u64> {
    let property = format!("written@{}", snapshot.name());
    zfs.property(snapshot.dataset(), &property)
        .inspect_err(|error| tracing::warn!(%error, %snapshot, "what was written is unknown"))
        .ok()?
        .value
        .parse()
        .ok()
}

fn rewind(zfs: &dyn Zfs, pool: &PoolName, options: ImportOptions) -> Review {
    let (pool_use, stops) = pool_use(zfs, pool, options.id);
    let checkpoint = if pool_use.this_host {
        zfs.checkpoint(pool)
            .inspect_err(|error| tracing::warn!(%error, %pool, "the checkpoint is unknown"))
            .ok()
            .flatten()
    } else {
        None
    };
    let since = checkpoint.as_ref().and_then(Checkpoint::created_at);
    let root: Option<DatasetName> = pool.as_str().parse().ok();
    let made = since.zip(root);
    let checkpoint = checkpoint.unwrap_or_default();
    let read_only = ImportOptions {
        access: Access::ReadOnly,
        rewind: false,
        ..options
    };
    Review {
        loses: vec![Loss::SinceCheckpoint {
            created: checkpoint.created,
            space: checkpoint.space,
            discarding: checkpoint.discarding,
            datasets: made
                .as_ref()
                .and_then(|(since, root)| datasets_since(zfs, root, *since)),
            snapshots: made
                .as_ref()
                .and_then(|(since, root)| snapshots_since(zfs, root, *since)),
        }],
        safer: Some(Safer::Import(Box::new(Action::Import(
            pool.clone(),
            read_only,
        )))),
        stops,
        pool_use: Some(pool_use),
        ..Review::default()
    }
}

/// The filesystems below and including `root` made at or after `since`,
/// in seconds since 1970.
fn datasets_since(zfs: &dyn Zfs, root: &DatasetName, since: u64) -> Option<Vec<DatasetName>> {
    let datasets = zfs
        .filesystems(Some(root), &["creation"])
        .inspect_err(|error| tracing::warn!(%error, %root, "the filesystems could not be listed"))
        .ok()?;
    Some(
        datasets
            .into_iter()
            .filter(|dataset| {
                dataset
                    .value("creation")
                    .and_then(|creation| creation.parse::<u64>().ok())
                    .is_some_and(|creation| creation >= since)
            })
            .map(|dataset| dataset.name)
            .collect(),
    )
}

/// The snapshots of `root` and its descendants taken at or after `since`,
/// in seconds since 1970.
fn snapshots_since(zfs: &dyn Zfs, root: &DatasetName, since: u64) -> Option<Vec<SnapshotName>> {
    let snapshots = zfs
        .snapshots(root)
        .inspect_err(|error| tracing::warn!(%error, %root, "the snapshots could not be listed"))
        .ok()?;
    Some(
        snapshots
            .into_iter()
            .filter(|snapshot| snapshot.creation >= since)
            .map(|snapshot| snapshot.name)
            .collect(),
    )
}

fn forced(zfs: &dyn Zfs, pool: &PoolName, options: ImportOptions) -> Review {
    let (pool_use, stops) = pool_use(zfs, pool, options.id);
    Review {
        undo: (!pool_use.this_host).then(|| Box::new(Action::Export(pool.clone()))),
        stops,
        pool_use: Some(pool_use),
        ..Review::default()
    }
}

/// Who else may use `pool`, the one whose identifier is `id` when given:
/// none but this host when it has it imported; otherwise the host its
/// configuration records, and whether another system has it imported
/// now, which stops its import. Another pool of that name imported here
/// is not it.
fn pool_use(zfs: &dyn Zfs, pool: &PoolName, id: Option<u64>) -> (PoolUse, Vec<Stop>) {
    let imported = zfs
        .pools()
        .inspect_err(|error| tracing::warn!(%error, "the imported pools could not be listed"))
        .is_ok_and(|pools| {
            pools
                .iter()
                .any(|imported| imported.name == *pool && id.is_none_or(|id| imported.guid == id))
        });
    if imported {
        let used = PoolUse {
            this_host: true,
            ..PoolUse::default()
        };
        return (used, Vec::new());
    }
    let listed = zfs
        .importable()
        .inspect_err(|error| tracing::warn!(%error, "the pools could not be scanned"))
        .unwrap_or_default();
    let found = listed
        .iter()
        .find(|listed| id.map_or(listed.name == *pool, |id| listed.id == id));
    let host = id.or(found.map(|found| found.id)).and_then(|id| {
        zfs.pool_host(id)
            .inspect_err(|error| tracing::warn!(%error, %pool, "the pool's host is unknown"))
            .ok()
    });
    let used = PoolUse {
        written: host.as_ref().and_then(|host| host.written),
        host: host.map(|host| PoolHost {
            hostid: host.hostid,
            hostname: host.hostname,
        }),
        this_host: false,
        active: found.is_some_and(|found| found.active),
    };
    let stops = if used.active {
        vec![Stop::Active {
            pool: pool.clone(),
            host: used.host.clone(),
        }]
    } else {
        Vec::new()
    };
    (used, stops)
}

#[cfg(test)]
mod tests;
