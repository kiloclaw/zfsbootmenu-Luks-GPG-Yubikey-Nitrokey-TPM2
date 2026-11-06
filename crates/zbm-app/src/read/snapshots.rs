//! The snapshots of a boot environment, from one listing of its own
//! (`zfs list -t snapshot -d 1`): the count and the newest, which every
//! snapshot of the machine holds, and the whole list, read when its column
//! opens (ADR 0016, point 6).

use std::io;

use zbm_types::DatasetName;
use zbm_zfs::Zfs;

use crate::model::{SnapshotInfo, SnapshotList};

/// How long the snapshots of one environment may be in a message to a
/// remote menu: half of a message ([`MAX_LENGTH`](crate::remote::MAX_LENGTH)),
/// as each snapshot of the machine read after the load carries them too,
/// with the rest of the machine. That is some 70,000 snapshots of names
/// as long as `autosnap_2026-11-06_00:00:01_daily`. The menu on the
/// console has the same limit, as it shows what one over SSH would.
pub(crate) const LOADED_LENGTH: usize = crate::remote::MAX_LENGTH / 2;

/// How many snapshots `environment` has of its own, and the newest; none
/// when they cannot be listed.
pub(crate) fn snapshot_summary(
    zfs: &dyn Zfs,
    environment: &DatasetName,
) -> (usize, Option<SnapshotInfo>) {
    let listed = zfs.own_snapshots(environment).unwrap_or_default();
    let newest = listed.last().map(|snapshot| SnapshotInfo {
        name: snapshot.name.clone(),
        created: snapshot.creation,
        used: snapshot.used,
    });
    (listed.len(), newest)
}

/// Every snapshot of `environment`, its descendants' left out, newest
/// first, with the space each holds; or why they are not read: they cannot
/// be listed, or take more than `limit` bytes in a message.
pub(crate) fn snapshot_list(
    zfs: &dyn Zfs,
    environment: &DatasetName,
    limit: usize,
) -> Result<SnapshotList, String> {
    let listed = zfs
        .own_snapshots(environment)
        .map_err(|error| error.to_string())?;
    let snapshots: Vec<SnapshotInfo> = listed
        .into_iter()
        .rev()
        .map(|snapshot| SnapshotInfo {
            name: snapshot.name,
            created: snapshot.creation,
            used: snapshot.used,
        })
        .collect();
    let list = SnapshotList {
        environment: environment.clone(),
        snapshots,
    };
    let length = encoded_length(&list);
    if length > limit {
        return Err(format!(
            "its {} snapshots take {length} bytes, more than the {limit} any menu is sent of \
             one environment",
            list.snapshots.len()
        ));
    }
    Ok(list)
}

/// The bytes `list` takes in a message, counted without keeping them.
fn encoded_length(list: &SnapshotList) -> usize {
    let mut counter = Counter(0);
    match serde_json::to_writer(&mut counter, list) {
        Ok(()) => counter.0,
        Err(_) => usize::MAX,
    }
}

/// A writer that counts what it is given, and keeps nothing.
struct Counter(usize);

impl io::Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
