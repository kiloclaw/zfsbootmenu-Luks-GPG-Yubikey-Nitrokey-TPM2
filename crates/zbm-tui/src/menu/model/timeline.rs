//! The workspace's Snapshots tab as a timeline (ADR 0007, decision 2; ADR
//! 0016, point 9): the snapshots of the column open, grouped by when they
//! were taken, the newest group first: today, each earlier day of this
//! week, each earlier week of this month, then each month. Weeks start on
//! Monday (ISO 8601), the first of a month on its first day, and the
//! calendar is UTC's, as the rest of the menu's dates; a snapshot taken
//! after now, by a clock ahead, is today's. Within a group the snapshots
//! keep the sort order.
//!
//! A group can be folded: it shows as its header alone, which is then the
//! one stop of its snapshots, and none of them is selected there. The
//! months are folded until the person opens one. The header of a group
//! not folded is no stop: the selection goes from snapshot to snapshot.
//! Each group knows where its stops start and the row of its header, so
//! that the view builds the rows of its window alone, of ten thousand
//! snapshots.

use std::collections::BTreeSet;

use super::{ListState, Model};
use crate::menu::filter::Ranked;
use crate::menu::format::{civil, size};
use crate::menu::world::SnapshotInfo;
use crate::text::text;

/// The seconds of a day.
const DAY: u64 = 86_400;

/// A group of the timeline, by the calendar, so that it stays the same
/// group, folded or not, from one snapshot of the machine to the next, on
/// the same day. Ordered oldest first: a month before a week, a week
/// before a day, a day before today.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Period {
    /// A month before this month and this week: its year and its month,
    /// from 1.
    Month(i64, i64),
    /// A week of this month before this week, by the day number (days
    /// since 1970-01-01) of its first day in this month: its Monday, or
    /// the first of the month.
    Week(i64),
    /// A day of this week before today, by its day number.
    Day(i64),
    /// Today, and the time after now.
    Today,
}

/// The day number of `seconds` since the epoch.
fn day_of(seconds: u64) -> i64 {
    i64::try_from(seconds / DAY).unwrap_or(i64::MAX)
}

/// The day of the week of the day number `day`: 0 for Monday, 6 for
/// Sunday. 1970-01-01 was a Thursday.
fn weekday(day: i64) -> i64 {
    (day + 3).rem_euclid(7)
}

/// The name of the month `month`, from 1, from the catalogue.
fn month_name(month: i64) -> String {
    text!("timeline-month-name", month = month)
}

impl Period {
    /// The group of a snapshot taken at `created`, at `now`, both in
    /// seconds since the epoch.
    pub(crate) fn of(now: u64, created: u64) -> Period {
        let today = day_of(now);
        let day = day_of(created);
        if day >= today {
            return Period::Today;
        }
        let monday = today - weekday(today);
        let (_, _, date) = civil(today);
        let first = today - (date - 1);
        if day >= monday {
            Period::Day(day)
        } else if day >= first {
            Period::Week((day - weekday(day)).max(first))
        } else {
            let (year, month, _) = civil(day);
            Period::Month(year, month)
        }
    }

    /// Whether its group is folded until the person opens it: a month's.
    fn is_folded_first(self) -> bool {
        matches!(self, Period::Month(..))
    }

    /// What its header says: `Today`, `Tuesday 20 October`, `Week of 12
    /// October`, `September 2026`.
    pub(crate) fn label(self) -> String {
        match self {
            Period::Today => text!("timeline-today"),
            Period::Day(day) => {
                let (_, month, date) = civil(day);
                text!(
                    "timeline-day",
                    weekday = text!("timeline-weekday-name", weekday = weekday(day) + 1),
                    day = date,
                    month = month_name(month)
                )
            }
            Period::Week(monday) => {
                let (_, month, date) = civil(monday);
                text!("timeline-week", day = date, month = month_name(month))
            }
            Period::Month(year, month) => text!(
                "timeline-month",
                month = month_name(month),
                year = year.to_string()
            ),
        }
    }
}

/// The groups the person folded or opened, which differ from how the
/// timeline first shows them, the months folded; for the day they were,
/// as the groups change with it: the next day, they are forgotten.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Folds {
    /// The day number of the day the groups are of.
    day: i64,
    /// The groups folded or opened.
    changed: BTreeSet<Period>,
}

impl Folds {
    /// Whether the group of `period` is folded.
    fn is_folded(&self, period: Period) -> bool {
        period.is_folded_first() != self.changed.contains(&period)
    }

    /// Folds the group of `period`, or opens it when it is folded.
    fn toggle(&mut self, period: Period) {
        if !self.changed.remove(&period) {
            self.changed.insert(period);
        }
    }

    /// The groups of `now`, in seconds since the epoch: on another day,
    /// they are as the timeline first shows them.
    fn on(&mut self, now: u64) {
        let day = day_of(now);
        if self.day != day {
            self.day = day;
            self.changed.clear();
        }
    }
}

/// A group of the timeline: its period, where its stops start in the
/// matches, and the row of its header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Group {
    /// When its snapshots were taken.
    pub(crate) period: Period,
    /// How many snapshots it holds.
    pub(crate) count: usize,
    /// The space they hold, each its own, in bytes.
    pub(crate) used: u64,
    /// Whether it shows as its header alone.
    pub(crate) folded: bool,
    /// The position in the matches of its first stop: its first snapshot,
    /// or its header when folded.
    first: usize,
    /// The row of its header.
    row: usize,
}

impl Group {
    /// How many stops it has: its snapshots, or its header when folded.
    fn stops(&self) -> usize {
        if self.folded { 1 } else { self.count }
    }

    /// How many rows it takes: its header, then its snapshots unless
    /// folded.
    fn rows(&self) -> usize {
        if self.folded { 1 } else { 1 + self.count }
    }

    /// What its header says after its period: how many snapshots it holds,
    /// the space they hold, and whether it is folded.
    pub(crate) fn facts(&self) -> Vec<String> {
        let mut facts = vec![text!("timeline-count", count = self.count), size(self.used)];
        if self.folded {
            facts.push(text!("timeline-folded"));
        }
        facts
    }
}

/// A row of the timeline as the view draws it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Row<'a> {
    /// The header of a group.
    Header(&'a Group),
    /// The snapshot of the match at this position.
    Snapshot(usize),
}

/// The groups of the timeline, newest first.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Timeline {
    groups: Vec<Group>,
}

impl Timeline {
    /// The timeline of `snapshots`, in their order within each group, at
    /// `now`, folded as `folds` say; and its matches: the stops, in its
    /// order.
    fn build(snapshots: &[SnapshotInfo], now: u64, folds: &Folds) -> (Timeline, Vec<Ranked>) {
        let mut keyed: Vec<(Period, usize)> = snapshots
            .iter()
            .enumerate()
            .map(|(index, snapshot)| (Period::of(now, snapshot.created), index))
            .collect();
        // A stable sort: each group keeps the sort order.
        keyed.sort_by_key(|(period, _)| std::cmp::Reverse(*period));
        let mut groups: Vec<Group> = Vec::new();
        let mut matches = Vec::with_capacity(keyed.len());
        let mut row = 0;
        let stop = |index| Ranked {
            index,
            positions: Vec::new(),
        };
        for chunk in keyed.chunk_by(|a, b| a.0 == b.0) {
            let Some(&(period, head)) = chunk.first() else {
                continue;
            };
            let used = chunk
                .iter()
                .filter_map(|(_, index)| snapshots.get(*index))
                .fold(0u64, |sum, snapshot| sum.saturating_add(snapshot.used));
            let group = Group {
                period,
                count: chunk.len(),
                used,
                folded: folds.is_folded(period),
                first: matches.len(),
                row,
            };
            if group.folded {
                matches.push(stop(head));
            } else {
                matches.extend(chunk.iter().map(|(_, index)| stop(*index)));
            }
            row += group.rows();
            groups.push(group);
        }
        (Timeline { groups }, matches)
    }

    /// The groups, newest first.
    #[cfg(test)]
    fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// The group of the stop at `position` of the matches.
    pub(crate) fn group_at(&self, position: usize) -> Option<&Group> {
        let after = self.groups.partition_point(|group| group.first <= position);
        let group = self.groups.get(after.checked_sub(1)?)?;
        (position < group.first + group.stops()).then_some(group)
    }

    /// How many rows the timeline takes.
    pub(crate) fn rows(&self) -> usize {
        self.groups
            .last()
            .map_or(0, |group| group.row + group.rows())
    }

    /// The row the stop at `position` is drawn at.
    pub(crate) fn row_of(&self, position: usize) -> usize {
        match self.group_at(position) {
            Some(group) if group.folded => group.row,
            Some(group) => group.row + 1 + (position - group.first),
            None => 0,
        }
    }

    /// The rows of the headers that are no stop, those of the groups not
    /// folded, in order: the line view does not number them.
    pub(crate) fn headers(&self) -> Vec<usize> {
        self.groups
            .iter()
            .filter(|group| !group.folded)
            .map(|group| group.row)
            .collect()
    }

    /// The rows from `first` on, `count` at most: only these are built.
    pub(crate) fn window(&self, first: usize, count: usize) -> Vec<Row<'_>> {
        let start = self
            .groups
            .partition_point(|group| group.row <= first)
            .saturating_sub(1);
        let mut rows = Vec::with_capacity(count.min(self.rows()));
        for group in self.groups.get(start..).unwrap_or_default() {
            let from = first.saturating_sub(group.row);
            for offset in from..group.rows() {
                if rows.len() == count {
                    return rows;
                }
                rows.push(match offset {
                    0 => Row::Header(group),
                    _ => Row::Snapshot(group.first + offset - 1),
                });
            }
        }
        rows
    }

    /// The position of the stop of `index`, a snapshot of the period
    /// `period`: its own, or its group's header when folded.
    fn position_of(&self, period: Period, index: usize, matches: &[Ranked]) -> Option<usize> {
        let group = self.groups.iter().find(|group| group.period == period)?;
        if group.folded {
            return Some(group.first);
        }
        let stops = matches.get(group.first..group.first + group.count)?;
        let at = stops.iter().position(|ranked| ranked.index == index)?;
        Some(group.first + at)
    }
}

/// Ranks `snapshots` into `list` as the timeline at `now` orders them,
/// folded as `folds` say, which the day forgets, and selects the one
/// called `name`, or its folded group; the timeline.
pub(super) fn rank(
    list: &mut ListState,
    snapshots: &[SnapshotInfo],
    now: u64,
    folds: &mut Folds,
    name: Option<&str>,
) -> Timeline {
    folds.on(now);
    let (timeline, matches) = Timeline::build(snapshots, now, folds);
    list.matches = matches;
    list.selected = list.selected.min(list.matches.len().saturating_sub(1));
    let named = name.and_then(|name| {
        let index = snapshots.iter().position(|s| s.name.name() == name)?;
        let period = Period::of(now, snapshots.get(index)?.created);
        timeline.position_of(period, index, &list.matches)
    });
    if let Some(position) = named {
        list.selected = position;
    }
    timeline
}

impl Model {
    /// The timeline of the snapshots column, in the workspace while no
    /// filter ranks the snapshots by how well each name matches.
    pub(crate) fn timeline(&self) -> Option<&Timeline> {
        self.timeline.as_ref()
    }

    /// The group of the timeline whose header is selected, as it is
    /// folded.
    pub(crate) fn folded_group(&self) -> Option<&Group> {
        self.timeline
            .as_ref()?
            .group_at(self.snapshots.selected)
            .filter(|group| group.folded)
    }

    /// Folds the group of the selected snapshot, or unfolds it when it is
    /// folded: the selection stays on it, as its header once folded, and
    /// on its first snapshot once unfolded. The group stays as it is in
    /// every environment's timeline until the day changes, which changes
    /// the groups. Nothing without the timeline.
    pub(crate) fn fold(&mut self) {
        let Some(group) = self
            .timeline
            .as_ref()
            .and_then(|timeline| timeline.group_at(self.snapshots.selected))
        else {
            return;
        };
        let period = group.period;
        self.folds.toggle(period);
        let name = self
            .snapshot_at_selection()
            .map(|snapshot| snapshot.name.name().to_owned());
        self.rank_again();
        self.rank_snapshots(name);
    }
}

#[cfg(test)]
mod tests;
