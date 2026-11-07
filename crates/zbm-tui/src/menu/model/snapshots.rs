//! The snapshots the menu read (ADR 0016, point 6): the lists of the
//! columns opened, which the machine sends when a column opens and again
//! with each snapshot of the machine after it, kept in the menu's sort
//! order; and what a column says while its list is not there.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use zbm_types::{DatasetName, SnapshotName};

use super::{Model, Sort, keep, refilter, timeline};
use crate::menu::keymap::Map;
use crate::menu::world::{Snapshot, SnapshotInfo, SnapshotList};

/// The seconds of a day.
const DAY: u64 = 86_400;

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
/// ([`Model::generation`]), the focus, the filter, the key map, whose
/// workspace groups them, and the day, by which it does.
pub(super) type Ranked = (u64, Option<DatasetName>, String, Map, u64);

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

/// The place of each snapshot of `lists` in its list, as the machine sent
/// it: newest first, in the order ZFS took them, by transaction group.
pub(super) fn ages(lists: &[SnapshotList]) -> BTreeMap<SnapshotName, usize> {
    let places = lists
        .iter()
        .flat_map(|list| list.snapshots.iter().enumerate());
    places.map(|(place, s)| (s.name.clone(), place)).collect()
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

    /// The selected snapshot; none on the header of a folded group of the
    /// timeline, which stands for its snapshots.
    pub fn selected_snapshot(&self) -> Option<&SnapshotInfo> {
        if self.folded_group().is_some() {
            return None;
        }
        self.snapshot_at_selection()
    }

    /// The snapshot of the selected match, which on a folded group of the
    /// timeline is the first of the group: what a refresh keeps selected.
    pub(super) fn snapshot_at_selection(&self) -> Option<&SnapshotInfo> {
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

    /// Forgets the marks of the snapshots that the list of their
    /// environment, read again, no longer has, so that no mark names a
    /// snapshot the menu does not show.
    pub(super) fn forget_unlisted_marks(&mut self) {
        let snapshot = &self.snapshot;
        self.marked.retain(|mark| {
            loaded_of(snapshot, mark.dataset())
                .is_none_or(|listed| listed.iter().any(|known| known.name == *mark))
        });
    }

    /// The marked snapshots, the oldest first: by when each was taken, and,
    /// taken in the same second, by the order ZFS took them in, as the
    /// machine's list says whatever the menu's sort. None while one of them
    /// is not in the column's list: the menu cannot tell its age.
    pub(crate) fn marked_by_age(&self) -> Option<Vec<&SnapshotName>> {
        let listed = self.focus_snapshots();
        let mut marked = Vec::with_capacity(self.marked.len());
        for mark in &self.marked {
            let created = listed.iter().find(|s| s.name == *mark)?.created;
            let place = *self.ages.get(mark)?;
            marked.push((created, Reverse(place), mark));
        }
        marked.sort_unstable();
        Some(marked.into_iter().map(|(_, _, mark)| mark).collect())
    }

    /// Takes `list`, which the machine read, in place of the list of its
    /// environment, keeping the snapshot selected by name, and the marks of
    /// the snapshots it still lists.
    pub(crate) fn take_loaded(&mut self, mut list: SnapshotList) {
        let selected = self.selected_names();
        if self.reading.as_ref() == Some(&list.environment) {
            self.reading = None;
        }
        if self.unread.as_ref() == Some(&list.environment) {
            self.unread = None;
        }
        self.ages
            .retain(|name, _| *name.dataset() != list.environment);
        self.ages.extend(ages(std::slice::from_ref(&list)));
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
        self.forget_unlisted_marks();
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

    /// Matches the filter against the snapshots of the focus again, in the
    /// workspace without one as its timeline orders them, and selects the
    /// one called `name`; only when the lists, the focus, the filter, the
    /// map or the clock's day changed since the last time, as ranking ten
    /// thousand snapshots takes longer than a frame.
    pub(super) fn rank_snapshots(&mut self, name: Option<String>) {
        let now = self.clock();
        let ranked = (
            self.generation,
            self.focus.clone(),
            self.snapshots.filter.clone(),
            self.map,
            now / DAY,
        );
        if self.ranked.as_ref() == Some(&ranked) {
            return;
        }
        let snapshots = focused_of(&self.snapshot, self.focus.as_ref());
        let grouped = self.map == Map::Workspace && self.snapshots.filter.is_empty();
        self.timeline = None;
        if grouped {
            let name = name.as_deref();
            let list = &mut self.snapshots;
            self.timeline = Some(timeline::rank(list, snapshots, now, &mut self.folds, name));
        } else {
            refilter(
                &mut self.snapshots,
                snapshots.iter().map(|snapshot| snapshot.name.name()),
            );
            keep(&mut self.snapshots, snapshots, name, |snapshot| {
                snapshot.name.name().to_owned()
            });
        }
        self.ranked = Some(ranked);
    }

    /// Forgets what the snapshots' matches were ranked from, so that the
    /// next refresh ranks them, as when their list state was reset.
    pub(crate) fn rank_again(&mut self) {
        self.ranked = None;
    }

    /// Groups the snapshots again once the clock passed midnight since
    /// the timeline grouped them, as its days are the clock's: yesterday's
    /// snapshots leave today's group, though the machine sent nothing.
    pub(crate) fn follow_clock(&mut self) {
        let day = self.clock() / DAY;
        let ranked = self.ranked.as_ref().map(|ranked| ranked.4);
        if self.timeline.is_some() && ranked.is_some_and(|ranked| ranked != day) {
            self.refresh();
        }
    }
}
