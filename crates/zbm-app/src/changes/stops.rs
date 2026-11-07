//! What stops a change that destroys snapshots, as ZFS would refuse it: a
//! clone of one of them, or a hold on one. A clone stops `zfs rollback -r`
//! before it destroys anything; a hold stops it only after it destroyed
//! every newer snapshot without one and every newer bookmark, so the
//! rollback checks first, and refuses both. A destroy checks first too,
//! and so names each.
//!
//! A read that fails stops nothing: ZFS still refuses what it would.

use zbm_types::SnapshotName;
use zbm_zfs::{SnapshotDetail, SnapshotOrBookmark, Zfs};

use crate::model::Stop;

/// The snapshots and bookmarks among `details`, those of the filesystem of
/// `snapshot`, that ZFS took after it, which `zfs rollback -r` destroys,
/// oldest first; `None` when `snapshot` is not among them.
pub(crate) fn newer<'d>(
    details: &'d [SnapshotDetail],
    snapshot: &SnapshotName,
) -> Option<Vec<&'d SnapshotDetail>> {
    let target = details
        .iter()
        .find(|detail| is_snapshot(detail, snapshot))?;
    Some(
        details
            .iter()
            .filter(|detail| detail.createtxg > target.createtxg)
            .collect(),
    )
}

/// Whether `detail` is of the snapshot `snapshot`, not of a bookmark.
pub(crate) fn is_snapshot(detail: &SnapshotDetail, snapshot: &SnapshotName) -> bool {
    matches!(&detail.name, SnapshotOrBookmark::Snapshot(name) if name == snapshot)
}

/// What stops destroying the snapshots of `details`: each clone of one,
/// then each hold on one, with its tag, which `zfs holds` reads; a hold
/// whose tag could not be read is named without it.
pub(crate) fn stops(zfs: &dyn Zfs, details: &[&SnapshotDetail]) -> Vec<Stop> {
    let snapshot = |detail: &SnapshotDetail| match &detail.name {
        SnapshotOrBookmark::Snapshot(name) => Some(name.clone()),
        SnapshotOrBookmark::Bookmark { .. } => None,
    };
    let mut found: Vec<Stop> = details
        .iter()
        .filter_map(|detail| Some((snapshot(detail)?, &detail.clones)))
        .flat_map(|(snapshot, clones)| {
            clones.iter().map(move |clone| Stop::Clone {
                snapshot: snapshot.clone(),
                clone: clone.clone(),
            })
        })
        .collect();
    let held: Vec<SnapshotName> = details
        .iter()
        .filter(|detail| detail.userrefs > 0)
        .filter_map(|detail| snapshot(detail))
        .collect();
    let holds = zfs.holds(&held).unwrap_or_else(|error| {
        tracing::warn!(%error, "the holds of the snapshots could not be read");
        Vec::new()
    });
    for snapshot in held {
        let tags: Vec<Stop> = holds
            .iter()
            .filter(|hold| hold.snapshot == snapshot)
            .map(|hold| Stop::Hold {
                snapshot: snapshot.clone(),
                tag: Some(hold.tag.clone()),
            })
            .collect();
        if tags.is_empty() {
            found.push(Stop::Hold {
                snapshot,
                tag: None,
            });
        } else {
            found.extend(tags);
        }
    }
    found
}

/// What stops rolling back to `snapshot`: the clones and holds of the
/// newer snapshots; none when they cannot be listed.
pub(crate) fn of_rollback(zfs: &dyn Zfs, snapshot: &SnapshotName) -> Vec<Stop> {
    let details = match zfs.snapshot_details(snapshot.dataset()) {
        Ok(details) => details,
        Err(error) => {
            tracing::warn!(%error, %snapshot, "the newer snapshots could not be listed");
            return Vec::new();
        }
    };
    newer(&details, snapshot).map_or_else(Vec::new, |newer| stops(zfs, &newer))
}

/// The details of `snapshots`, of one filesystem, among `details`, in the
/// order ZFS took them; those it does not list are left out.
pub(crate) fn named<'d>(
    details: &'d [SnapshotDetail],
    snapshots: &[SnapshotName],
) -> Vec<&'d SnapshotDetail> {
    details
        .iter()
        .filter(|detail| {
            snapshots
                .iter()
                .any(|snapshot| is_snapshot(detail, snapshot))
        })
        .collect()
}

/// What stops destroying `snapshots`, of one filesystem: their clones and
/// holds; none when they cannot be listed.
pub(crate) fn of_destroy(zfs: &dyn Zfs, snapshots: &[SnapshotName]) -> Vec<Stop> {
    let Some(first) = snapshots.first() else {
        return Vec::new();
    };
    match zfs.snapshot_details(first.dataset()) {
        Ok(details) => stops(zfs, &named(&details, snapshots)),
        Err(error) => {
            tracing::warn!(%error, %first, "the snapshots could not be listed");
            Vec::new()
        }
    }
}

/// `stop` in words.
pub(crate) fn why(stop: &Stop) -> String {
    match stop {
        Stop::Clone { snapshot, clone } => {
            format!("{snapshot} has the clone {}", clone.printable())
        }
        Stop::Hold {
            snapshot,
            tag: Some(tag),
        } => format!("{snapshot} is held by {}", tag.printable()),
        Stop::Hold {
            snapshot,
            tag: None,
        } => format!("{snapshot} is held"),
        Stop::Active { pool, host } => {
            let hostname = host.as_ref().and_then(|host| host.hostname.as_ref());
            match hostname {
                Some(hostname) => {
                    format!("{pool} is imported by {} now", hostname.printable())
                }
                None => format!("{pool} is imported by another system now"),
            }
        }
    }
}

#[cfg(test)]
mod tests;
