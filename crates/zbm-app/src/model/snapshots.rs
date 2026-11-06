//! The snapshots of a boot environment, which the menu reads when their
//! column opens (ADR 0016, point 6).

use zbm_types::{DatasetName, SnapshotName};

/// A snapshot of a boot environment.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotInfo {
    /// Its name.
    pub name: SnapshotName,
    /// When it was taken, in seconds since the epoch.
    pub created: u64,
    /// The space only it holds, in bytes.
    pub used: u64,
}

/// The snapshots of one boot environment, every one of them, newest first:
/// what [`Action::LoadSnapshots`](super::Action::LoadSnapshots) reads, and
/// what each snapshot of the machine read after it carries for its column
/// ([`Snapshot::loaded`](super::Snapshot::loaded)).
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotList {
    /// The environment.
    pub environment: DatasetName,
    /// Its snapshots, its descendants' left out, newest first.
    pub snapshots: Vec<SnapshotInfo>,
}
