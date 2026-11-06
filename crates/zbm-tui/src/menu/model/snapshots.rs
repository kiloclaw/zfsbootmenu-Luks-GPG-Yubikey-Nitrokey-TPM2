//! The snapshots the menu read (ADR 0016, point 6): the lists of the
//! columns opened, which the machine sends when a column opens and again
//! with each snapshot of the machine after it, kept in the menu's sort
//! order; and what a column says while its list is not there.

use zbm_types::DatasetName;

use super::{Model, Sort, keep, refilter};
use crate::menu::world::{Snapshot, SnapshotInfo, SnapshotList};

/// What the snapshots column of the focused environment shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Column {
    /// Its snapshots, read.
    Loaded,
    /// They are asked for, and the machine has not sent them yet.
    Reading,
    /// They are not read: the load was cancelled or failed, or the
    /// countdown ran when the column opened.
    NotRead,
}

/// What the snapshots' matches were ranked from: the lists as they were
/// ([`Model::generation`]), the focus and the filter.
pub(super) type Ranked = (u64, Option<DatasetName>, String);

/// Sorts `snapshots` in the order `sort` says; equal ones keep theirs.
fn sort_snapshots(snapshots: &mut [SnapshotInfo], sort: Sort) {
    match sort {
        Sort::Name => snapshots.sort_by(|a, b| a.name.name().cmp(b.name.name())),
        Sort::Creation => snapshots.sort_by_key(|s| std::cmp::Reverse(s.created)),
        Sort::Size => snapshots.sort_by_key(|s| std::cmp::Reverse(s.used)),
    }
}

/// Sorts every list of `lists` in the order `sort` says, once, as they
/// come: the views read them as they are.
pub(super) fn sort_lists(lists: &mut [SnapshotList], sort: Sort) {
    for list in lists {
        sort_snapshots(&mut list.snapshots, sort);
    }
}

/// The snapshots of `environment` that `snapshot` holds, if they are read.
fn loaded_of<'a>(snapshot: &'a Snapshot, environment: &DatasetName) -> Option<&'a [SnapshotInfo]> {
    snapshot
        .loaded
        .iter()
        .find(|list| list.environment == *environment)
        .map(|list| list.snapshots.as_slice())
}

/// The snapshots of `focus` that `snapshot` holds; none when they are not
/// read.
fn focused_of<'a>(snapshot: &'a Snapshot, focus: Option<&DatasetName>) -> &'a [SnapshotInfo] {
    focus
        .and_then(|focus| loaded_of(snapshot, focus))
        .unwrap_or_default()
}

impl Model {
    /// The snapshots of `environment` the machine sent, in the sort order;
    /// `None` when they are not read.
    pub(crate) fn loaded_snapshots(&self, environment: &DatasetName) -> Option<&[SnapshotInfo]> {
        loaded_of(&self.snapshot, environment)
    }

    /// The snapshots of the focused environment, in the sort order; none
    /// while they are not read, as their column says.
    pub fn focus_snapshots(&self) -> &[SnapshotInfo] {
        focused_of(&self.snapshot, self.focus.as_ref())
    }

    /// The selected snapshot.
    pub fn selected_snapshot(&self) -> Option<&SnapshotInfo> {
        self.focus_snapshots().get(self.snapshots.selected_item()?)
    }

    /// What the snapshots column of the focused environment shows.
    pub(crate) fn column(&self) -> Column {
        let Some(focus) = &self.focus else {
            return Column::NotRead;
        };
        if self.loaded_snapshots(focus).is_some() {
            Column::Loaded
        } else if self.reading.as_ref() == Some(focus) {
            Column::Reading
        } else {
            Column::NotRead
        }
    }

    /// Whether a snapshot of the machine asks for the snapshots of the
    /// column again: they are not read, its environment is still there,
    /// and they were not read because the load was cancelled, not because
    /// it failed, which it would again.
    pub(crate) fn needs_load(&self) -> bool {
        self.column() == Column::NotRead
            && self.focused().is_some()
            && self.unread.as_ref() != self.focus.as_ref()
    }

    /// Takes `list`, which the machine read, in place of the list of its
    /// environment, keeping the snapshot selected by name.
    pub(crate) fn take_loaded(&mut self, mut list: SnapshotList) {
        let selected = self.selected_names();
        if self.reading.as_ref() == Some(&list.environment) {
            self.reading = None;
        }
        if self.unread.as_ref() == Some(&list.environment) {
            self.unread = None;
        }
        sort_snapshots(&mut list.snapshots, self.sort);
        let known = self
            .snapshot
            .loaded
            .iter_mut()
            .find(|known| known.environment == list.environment);
        match known {
            Some(known) if *known == list => return,
            Some(known) => *known = list,
            None => self.snapshot.loaded.push(list),
        }
        self.generation = self.generation.wrapping_add(1);
        self.refresh_keeping(None, selected);
    }

    /// The machine could not read the snapshots of `environment`: their
    /// column says they are not read, until it is opened again.
    pub(crate) fn not_readable(&mut self, environment: DatasetName) {
        if self.reading.as_ref() == Some(&environment) {
            self.reading = None;
        }
        self.unread = Some(environment);
    }

    /// The machine ended all it was sent, and the snapshots asked for did
    /// not come, as when Escape cancelled their load, or the session
    /// refused it: their column says they are not read.
    pub(crate) fn not_loaded(&mut self) {
        self.reading = None;
    }

    /// Matches the filter against the snapshots of the focus again, and
    /// selects the one called `name`; only when the lists, the focus or
    /// the filter changed since the last time, as ranking ten thousand
    /// snapshots takes longer than a frame.
    pub(super) fn rank_snapshots(&mut self, name: Option<String>) {
        let ranked = (
            self.generation,
            self.focus.clone(),
            self.snapshots.filter.clone(),
        );
        if self.ranked.as_ref() == Some(&ranked) {
            return;
        }
        let snapshots = focused_of(&self.snapshot, self.focus.as_ref());
        refilter(
            &mut self.snapshots,
            snapshots.iter().map(|snapshot| snapshot.name.name()),
        );
        keep(&mut self.snapshots, snapshots, name, |snapshot| {
            snapshot.name.name().to_owned()
        });
        self.ranked = Some(ranked);
    }

    /// Forgets what the snapshots' matches were ranked from, so that the
    /// next refresh ranks them, as when their list state was reset.
    pub(crate) fn rank_again(&mut self) {
        self.ranked = None;
    }
}
