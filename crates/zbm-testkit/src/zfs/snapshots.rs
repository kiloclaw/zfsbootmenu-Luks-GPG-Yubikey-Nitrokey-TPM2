//! The snapshots and bookmarks of [`FakeZfs`] and what keeps them: their
//! builders, and the reads of the port that list them.
//!
//! The fake's `createtxg` is the place of a snapshot in the order of
//! (creation, name), from 1: snapshots taken in the same second were taken
//! in the order of their names. A bookmark shares the place of the
//! snapshot it was made from. The first change fixes them, as ZFS never
//! changes a `createtxg`: a snapshot destroyed moves no other, and one
//! made later comes after every other. Every snapshot holds 4 KiB of its
//! own.
//!
//! ```
//! use zbm_testkit::zfs::FakeZfs;
//! use zbm_zfs::Zfs;
//!
//! let zfs = FakeZfs::new()
//!     .pool("zroot")
//!     .filesystem("zroot/ROOT/void", &[])
//!     .snapshot("zroot/ROOT/void@a", 100)
//!     .snapshot("zroot/ROOT/void@b", 100)
//!     .bookmark("zroot/ROOT/void@a", "zroot/ROOT/void#a")
//!     .hold("zroot/ROOT/void@b", "keep");
//! let details = zfs.snapshot_details(&"zroot/ROOT/void".parse()?)?;
//! let txgs: Vec<u64> = details.iter().map(|found| found.createtxg).collect();
//! assert_eq!(txgs, [1, 1, 2]);
//! assert_eq!(details[1].name.to_string(), "zroot/ROOT/void#a");
//! assert_eq!(details[2].userrefs, 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use zbm_types::{DatasetName, SnapshotName, Untrusted};
use zbm_zfs::{
    Hold, Property, Snapshot, SnapshotDetail, SnapshotOrBookmark, SnapshotSet, SnapshotSpace,
    ZfsError,
};

use super::{FakeZfs, State, name};

/// The space each snapshot holds on its own.
const USED: u64 = 4096;

/// Where a snapshot is in the order ZFS took them: its creation and name.
pub(super) type Taken = (u64, SnapshotName);

impl FakeZfs {
    /// Adds the snapshot `snapshot`, taken at `creation` seconds.
    #[must_use]
    pub fn snapshot(self, snapshot: &str, creation: u64) -> FakeZfs {
        {
            let mut state = self.state.borrow_mut();
            let snapshot: SnapshotName = name(snapshot);
            state.snapshots.insert(snapshot.clone(), creation);
            if state.txgs.is_some() {
                number(&mut state, (creation, snapshot), None);
            }
        }
        self
    }

    /// Makes the filesystem `dataset` a clone of `snapshot`.
    #[must_use]
    pub fn origin(self, dataset: &str, snapshot: &str) -> FakeZfs {
        self.state
            .borrow_mut()
            .origins
            .insert(name(dataset), name(snapshot));
        self
    }

    /// Places a hold tagged `tag` on `snapshot`, which keeps it from being
    /// destroyed.
    #[must_use]
    pub fn hold(self, snapshot: &str, tag: &str) -> FakeZfs {
        self.state
            .borrow_mut()
            .holds
            .entry(name(snapshot))
            .or_default()
            .push(tag.to_owned());
        self
    }

    /// Adds the bookmark `bookmark`, such as `zroot/ROOT/void#a`, of the
    /// snapshot `snapshot`, which must exist.
    #[must_use]
    pub fn bookmark(self, snapshot: &str, bookmark: &str) -> FakeZfs {
        {
            let mut state = self.state.borrow_mut();
            let snapshot: SnapshotName = name(snapshot);
            let creation = *state
                .snapshots
                .get(&snapshot)
                .unwrap_or_else(|| panic!("FakeZfs has no snapshot {snapshot}"));
            let marked = name(&bookmark.replacen('#', "@", 1));
            state.bookmarks.insert(marked, (creation, snapshot));
        }
        self
    }
}

/// The snapshots of `dataset` and its descendants.
pub(super) fn snapshots(state: &State, dataset: &DatasetName) -> Result<Vec<Snapshot>, ZfsError> {
    exists(state, dataset)?;
    Ok(state
        .snapshots
        .iter()
        .filter(|(snapshot, _)| {
            let of = snapshot.dataset();
            of == dataset
                || of
                    .as_str()
                    .strip_prefix(dataset.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
        .map(|(name, creation)| Snapshot {
            name: name.clone(),
            creation: *creation,
        })
        .collect())
}

/// The snapshots of `dataset` itself, oldest first.
pub(super) fn own_snapshots(
    state: &State,
    dataset: &DatasetName,
) -> Result<Vec<SnapshotSpace>, ZfsError> {
    exists(state, dataset)?;
    let mut own: Vec<SnapshotSpace> = state
        .snapshots
        .iter()
        .filter(|(snapshot, _)| snapshot.dataset() == dataset)
        .map(|(name, creation)| SnapshotSpace {
            name: name.clone(),
            creation: *creation,
            used: USED,
        })
        .collect();
    own.sort_by_key(|snapshot| snapshot.creation);
    Ok(own)
}

/// A property of `snapshot`: its `creation`, or its filesystem's.
pub(super) fn snapshot_property(
    state: &State,
    snapshot: &SnapshotName,
    property: &str,
) -> Result<Property, ZfsError> {
    let creation = state
        .snapshots
        .get(snapshot)
        .ok_or_else(|| ZfsError::Missing(snapshot.to_string()))?;
    Ok(match property {
        "creation" => Property::new(creation.to_string(), zbm_zfs::Source::None),
        _ => FakeZfs::resolve(state, snapshot.dataset(), property),
    })
}

/// The `createtxg` of each place in the order ZFS took the snapshots, the
/// bookmarks' snapshots included, from 1.
fn ranks(state: &State) -> BTreeMap<Taken, u64> {
    let taken: BTreeSet<Taken> = state
        .snapshots
        .iter()
        .map(|(snapshot, creation)| (*creation, snapshot.clone()))
        .chain(state.bookmarks.values().cloned())
        .collect();
    taken.into_iter().zip(1..).collect()
}

/// The `createtxg` of each place a snapshot was taken at: its rank until a
/// change fixes them.
fn txgs(state: &State) -> Cow<'_, BTreeMap<Taken, u64>> {
    match &state.txgs {
        Some(fixed) => Cow::Borrowed(fixed),
        None => Cow::Owned(ranks(state)),
    }
}

/// Fixes the `createtxg` of every snapshot and bookmark, before a change
/// that would move their ranks.
pub(super) fn fix(state: &mut State) {
    if state.txgs.is_none() {
        state.txgs = Some(ranks(state));
    }
}

/// Gives the snapshot taken at `taken` the `createtxg` `txg`, or the next
/// one after every other: a change made it.
pub(super) fn number(state: &mut State, taken: Taken, txg: Option<u64>) {
    fix(state);
    if let Some(fixed) = state.txgs.as_mut() {
        let next = fixed.values().max().map_or(1, |latest| latest + 1);
        fixed.insert(taken, txg.unwrap_or(next));
    }
}

/// The `createtxg` of `snapshot`, which exists, among `txgs`.
fn txg(state: &State, txgs: &BTreeMap<Taken, u64>, snapshot: &SnapshotName) -> u64 {
    let creation = state.snapshots.get(snapshot).copied().unwrap_or_default();
    txgs.get(&(creation, snapshot.clone()))
        .copied()
        .unwrap_or_default()
}

/// The snapshots and bookmarks of `dataset` itself, in the order ZFS took
/// them, a snapshot before its bookmark.
pub(super) fn snapshot_details(
    state: &State,
    dataset: &DatasetName,
) -> Result<Vec<SnapshotDetail>, ZfsError> {
    exists(state, dataset)?;
    let txgs = txgs(state);
    let snapshots = state
        .snapshots
        .keys()
        .filter(|snapshot| snapshot.dataset() == dataset)
        .map(|snapshot| SnapshotDetail {
            name: SnapshotOrBookmark::Snapshot(snapshot.clone()),
            createtxg: txg(state, &txgs, snapshot),
            used: Some(USED),
            userrefs: state
                .holds
                .get(snapshot)
                .map_or(0, |tags| count(tags.len())),
            clones: state
                .origins
                .iter()
                .filter(|(_, origin)| *origin == snapshot)
                .map(|(clone, _)| Untrusted::new(clone.to_string()))
                .collect(),
        });
    let bookmarks = state
        .bookmarks
        .iter()
        .filter(|(bookmark, _)| bookmark.dataset() == dataset)
        .map(|(bookmark, taken)| SnapshotDetail {
            name: SnapshotOrBookmark::Bookmark {
                dataset: dataset.clone(),
                name: bookmark.name().to_owned(),
            },
            createtxg: txgs.get(taken).copied().unwrap_or_default(),
            used: None,
            userrefs: 0,
            clones: Vec::new(),
        });
    let mut details: Vec<SnapshotDetail> = snapshots.chain(bookmarks).collect();
    details.sort_by_key(|detail| {
        let bookmark = matches!(detail.name, SnapshotOrBookmark::Bookmark { .. });
        (detail.createtxg, bookmark)
    });
    Ok(details)
}

/// The holds on `snapshots`, which must exist.
pub(super) fn holds(state: &State, snapshots: &[SnapshotName]) -> Result<Vec<Hold>, ZfsError> {
    let mut found = Vec::new();
    for snapshot in snapshots {
        if !state.snapshots.contains_key(snapshot) {
            let names: Vec<String> = snapshots.iter().map(ToString::to_string).collect();
            return Err(ZfsError::Failed {
                command: format!("zfs holds -H -p {}", names.join(" ")),
                status: 1,
                message: format!("cannot open '{snapshot}': dataset does not exist"),
            });
        }
        let tags = state.holds.get(snapshot).into_iter().flatten();
        found.extend(tags.map(|tag| Hold {
            snapshot: snapshot.clone(),
            tag: Untrusted::new(tag.clone()),
        }));
    }
    Ok(found)
}

/// The snapshots of the filesystem of `snapshot` that ZFS took after it,
/// by `createtxg`.
pub(super) fn later(state: &State, snapshot: &SnapshotName) -> Vec<SnapshotName> {
    let txgs = txgs(state);
    let at = txg(state, &txgs, snapshot);
    state
        .snapshots
        .keys()
        .filter(|other| other.dataset() == snapshot.dataset() && txg(state, &txgs, other) > at)
        .cloned()
        .collect()
}

/// The bookmarks of the filesystem of `snapshot` that ZFS made after it,
/// which `zfs rollback -r` destroys with the later snapshots.
pub(super) fn later_bookmarks(state: &State, snapshot: &SnapshotName) -> Vec<SnapshotName> {
    let txgs = txgs(state);
    let at = txg(state, &txgs, snapshot);
    state
        .bookmarks
        .iter()
        .filter(|(bookmark, from)| {
            bookmark.dataset() == snapshot.dataset()
                && txgs.get(*from).is_some_and(|made| *made > at)
        })
        .map(|(bookmark, _)| bookmark.clone())
        .collect()
}

/// The snapshots `set` names, as `zfs destroy` finds them: each that
/// exists, a name that does not being passed over, or a range's. They must
/// be of one dataset, and at least one must exist.
pub(super) fn named(
    state: &State,
    set: &SnapshotSet,
    command: &str,
) -> Result<Vec<SnapshotName>, ZfsError> {
    let failed = |message: &str| ZfsError::Failed {
        command: command.to_owned(),
        status: 1,
        message: message.to_owned(),
    };
    let named = match set {
        SnapshotSet::Each(snapshots) => snapshots.clone(),
        SnapshotSet::Range { first, last } => vec![first.clone(), last.clone()],
    };
    let Some(dataset) = named.first().map(SnapshotName::dataset) else {
        return Err(failed("missing dataset argument"));
    };
    if named.iter().any(|snapshot| snapshot.dataset() != dataset) {
        return Err(failed("too many arguments"));
    }
    let mut found: Vec<SnapshotName> = Vec::new();
    let existing = match set {
        SnapshotSet::Each(snapshots) => snapshots
            .iter()
            .filter(|snapshot| state.snapshots.contains_key(*snapshot))
            .cloned()
            .collect(),
        SnapshotSet::Range { first, last } => range(state, first, last),
    };
    for snapshot in existing {
        if !found.contains(&snapshot) {
            found.push(snapshot);
        }
    }
    if found.is_empty() {
        return Err(failed(
            "could not find any snapshots to destroy; check snapshot names.",
        ));
    }
    Ok(found)
}

/// The snapshots of the dataset of `first` from it to `last`, in the order
/// ZFS took them, as `zfs` walks a range: none when `last` does not exist
/// or comes before `first`, or `first` does not exist.
fn range(state: &State, first: &SnapshotName, last: &SnapshotName) -> Vec<SnapshotName> {
    if !state.snapshots.contains_key(last) {
        return Vec::new();
    }
    let txgs = txgs(state);
    let mut own: Vec<(u64, &SnapshotName)> = state
        .snapshots
        .keys()
        .filter(|snapshot| snapshot.dataset() == first.dataset())
        .map(|snapshot| (txg(state, &txgs, snapshot), snapshot))
        .collect();
    own.sort();
    let mut found = Vec::new();
    let mut seen_first = false;
    for (_, snapshot) in own {
        seen_first |= snapshot == first;
        if seen_first {
            found.push(snapshot.clone());
        }
        if snapshot == last {
            break;
        }
    }
    found
}

/// Whether a hold keeps `snapshot`.
pub(super) fn is_held(state: &State, snapshot: &SnapshotName) -> bool {
    state
        .holds
        .get(snapshot)
        .is_some_and(|tags| !tags.is_empty())
}

/// The first of `snapshots` that has a clone, and the first with a hold.
pub(super) fn kept(
    state: &State,
    snapshots: &[SnapshotName],
) -> (Option<SnapshotName>, Option<SnapshotName>) {
    let cloned = snapshots
        .iter()
        .find(|snapshot| state.origins.values().any(|origin| origin == *snapshot));
    let held = snapshots.iter().find(|snapshot| is_held(state, snapshot));
    (cloned.cloned(), held.cloned())
}

/// The space destroying `set` would free, as `zfs destroy -n` counts it:
/// it refuses a snapshot with clones but, destroying nothing, not one
/// with holds.
pub(super) fn destroy_space(state: &State, set: &SnapshotSet) -> Result<u64, ZfsError> {
    let command = format!("zfs destroy -n -v -p {}", shown(set));
    if let Some(message) = state.failures.get("destroy_space") {
        return Err(ZfsError::Failed {
            command,
            status: 1,
            message: message.clone(),
        });
    }
    let named = named(state, set, &command)?;
    if let (Some(cloned), _) = kept(state, &named) {
        return Err(ZfsError::Failed {
            command,
            status: 1,
            message: format!("cannot destroy snapshot {cloned}: snapshot has dependent clones"),
        });
    }
    Ok(USED.saturating_mul(count(named.len())))
}

/// `count` as the number ZFS gives.
fn count(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

/// `set` as `zfs destroy` names it: `<dataset>@a,b` or `<dataset>@a%b`.
pub(super) fn shown(set: &SnapshotSet) -> String {
    match set {
        SnapshotSet::Each(snapshots) => {
            let mut names = snapshots.iter();
            let first = names.next().map(ToString::to_string).unwrap_or_default();
            names.fold(first, |shown, snapshot| {
                format!("{shown},{}", snapshot.name())
            })
        }
        SnapshotSet::Range { first, last } => format!("{first}%{}", last.name()),
    }
}

/// Fails with [`ZfsError::Missing`] unless `dataset` exists.
fn exists(state: &State, dataset: &DatasetName) -> Result<(), ZfsError> {
    if state.datasets.contains_key(dataset) {
        Ok(())
    } else {
        Err(ZfsError::Missing(dataset.to_string()))
    }
}

#[cfg(test)]
mod tests;
