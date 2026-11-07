//! What a review of a change to snapshots reads: each snapshot and bookmark
//! of a dataset in the order ZFS took them, the holds on snapshots, and the
//! snapshots one `zfs destroy` names.
//!
//! Clone names and hold tags come from the pool, which whoever made it
//! chose: they are [`Untrusted`].

use std::fmt;

use zbm_types::{DatasetName, SnapshotName, Untrusted};

/// What a [`SnapshotDetail`] is. A bookmark often shares its dataset and
/// short name with a snapshot, but it is not one: no [`SnapshotName`]
/// names it, so nothing can destroy the snapshot when it meant the
/// bookmark.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotOrBookmark {
    /// A snapshot.
    Snapshot(SnapshotName),
    /// A bookmark, `<dataset>#<name>`, which holds no data and has no
    /// clones or holds.
    Bookmark {
        /// The dataset it is of.
        dataset: DatasetName,
        /// Its own name: the part after `#`, of the characters a
        /// snapshot's own name has.
        name: String,
    },
}

impl fmt::Display for SnapshotOrBookmark {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotOrBookmark::Snapshot(snapshot) => write!(formatter, "{snapshot}"),
            SnapshotOrBookmark::Bookmark { dataset, name } => {
                write!(formatter, "{dataset}#{name}")
            }
        }
    }
}

/// A snapshot or a bookmark of a dataset, as `zfs list -t
/// snapshot,bookmark -o name,createtxg,used,userrefs,clones` lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotDetail {
    /// The snapshot or the bookmark.
    pub name: SnapshotOrBookmark,
    /// The transaction group that created it: ZFS's own order, which
    /// tells apart snapshots taken in the same second.
    pub createtxg: u64,
    /// The space only it holds (`used`), in bytes; `None` for a bookmark.
    pub used: Option<u64>,
    /// The number of holds on it (`userrefs`), which keep it from being
    /// destroyed.
    pub userrefs: u64,
    /// The filesystems cloned from it, which keep it from being destroyed.
    pub clones: Vec<Untrusted<String>>,
}

/// A hold on a snapshot, as `zfs holds` lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hold {
    /// The snapshot held.
    pub snapshot: SnapshotName,
    /// The tag whoever placed the hold gave it.
    pub tag: Untrusted<String>,
}

/// Snapshots of one dataset that one `zfs destroy` names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotSet {
    /// These snapshots: `<dataset>@a,b,c`.
    Each(Vec<SnapshotName>),
    /// The first, the last and every snapshot ZFS took between them, as a
    /// rollback destroys them: `<dataset>@first%last`.
    Range {
        /// The oldest.
        first: SnapshotName,
        /// The newest.
        last: SnapshotName,
    },
}
