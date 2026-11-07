//! The groups of the timeline at their boundaries, in UTC: midnight, the
//! start of an ISO week (a Monday), the start of a month and of a year, a
//! snapshot taken after now and one at the epoch; and the rows the view
//! builds of it.

use pretty_assertions::assert_eq;

use super::*;

/// The seconds of an hour.
const HOUR: u64 = 3600;

/// The day number of 2026-10-22, a Thursday.
const OCTOBER_22: i64 = 20_748;

/// The seconds since the epoch at the start of the day `day` days after
/// 2026-10-22, before it when negative.
fn midnight(day: i64) -> u64 {
    u64::try_from(OCTOBER_22 + day).unwrap() * DAY
}

/// The snapshot `name` of `zroot/ROOT/void`, taken at `created`, holding
/// `used` bytes.
fn snapshot(name: &str, created: u64, used: u64) -> SnapshotInfo {
    SnapshotInfo {
        name: format!("zroot/ROOT/void@{name}").parse().unwrap(),
        created,
        used,
    }
}

#[test]
fn a_day_starts_at_midnight_utc_and_a_snapshot_after_now_is_today_s() {
    let now = midnight(0);

    assert_eq!(Period::of(now, now), Period::Today);
    assert_eq!(Period::of(now + 10 * HOUR, now), Period::Today);
    assert_eq!(Period::of(now, now - 1), Period::Day(OCTOBER_22 - 1));
    // A clock ahead: the snapshot was taken tomorrow.
    assert_eq!(Period::of(now, now + 30 * HOUR), Period::Today);
    assert_eq!(weekday(OCTOBER_22), 3);
}

#[test]
fn weeks_start_on_monday_as_iso_8601_says() {
    // Monday 19 October, ten in the morning.
    let monday = midnight(-3) + 10 * HOUR;

    assert_eq!(Period::of(monday, midnight(-3)), Period::Today);
    // Sunday 18 October is in the week of Monday 12 October.
    assert_eq!(
        Period::of(monday, midnight(-3) - 1),
        Period::Week(OCTOBER_22 - 10)
    );
    assert_eq!(
        Period::of(monday, midnight(-10)),
        Period::Week(OCTOBER_22 - 10)
    );
    assert_eq!(
        Period::of(monday, midnight(-10) - 1),
        Period::Week(OCTOBER_22 - 17)
    );
    // Thursday 22 October: Monday to Wednesday are days of this week.
    let thursday = midnight(0) + HOUR;
    assert_eq!(
        Period::of(thursday, midnight(-3)),
        Period::Day(OCTOBER_22 - 3)
    );
    assert_eq!(
        Period::of(thursday, midnight(-3) - 1),
        Period::Week(OCTOBER_22 - 10)
    );
}

#[test]
fn a_month_starts_on_its_first_day_and_this_week_comes_before_it() {
    // Thursday 1 October: this week started on Monday 28 September.
    let first = midnight(-21) + HOUR;
    assert_eq!(
        Period::of(first, midnight(-22)),
        Period::Day(OCTOBER_22 - 22)
    );
    assert_eq!(
        Period::of(first, midnight(-24)),
        Period::Day(OCTOBER_22 - 24)
    );
    assert_eq!(Period::of(first, midnight(-24) - 1), Period::Month(2026, 9));

    // Friday 9 October: the week of 28 September is split, its days of
    // October in a week of this month, which starts on the first, those
    // of September in its month.
    let ninth = midnight(-13) + HOUR;
    assert_eq!(
        Period::of(ninth, midnight(-18)),
        Period::Week(OCTOBER_22 - 21)
    );
    assert_eq!(
        Period::of(ninth, midnight(-21)),
        Period::Week(OCTOBER_22 - 21)
    );
    assert_eq!(Period::of(ninth, midnight(-21) - 1), Period::Month(2026, 9));
}

#[test]
fn a_year_starts_on_the_first_of_january() {
    // Saturday 2 January 2027: this week started on Monday 28 December.
    let new_year = midnight(72) + HOUR;
    assert_eq!(
        Period::of(new_year, midnight(70)),
        Period::Day(OCTOBER_22 + 70)
    );
    assert_eq!(
        Period::of(new_year, midnight(67)),
        Period::Day(OCTOBER_22 + 67)
    );
    assert_eq!(Period::of(new_year, midnight(66)), Period::Month(2026, 12));
    assert_eq!(Period::of(new_year, midnight(-1)), Period::Month(2026, 10));
}

#[test]
fn a_snapshot_at_the_epoch_is_in_its_month_and_today_s_at_it() {
    assert_eq!(Period::of(midnight(0), 0), Period::Month(1970, 1));
    assert_eq!(Period::of(0, 0), Period::Today);
    assert_eq!(Period::of(u64::MAX, 0), Period::Month(1970, 1));
    assert_eq!(Period::of(u64::MAX, u64::MAX), Period::Today);
}

#[test]
fn the_headers_say_when_from_the_catalogue() {
    assert_eq!(Period::Today.label(), "Today");
    assert_eq!(Period::Day(OCTOBER_22 - 1).label(), "Wednesday 21 October");
    assert_eq!(Period::Day(OCTOBER_22 + 3).label(), "Sunday 25 October");
    assert_eq!(Period::Week(OCTOBER_22 - 10).label(), "Week of 12 October");
    assert_eq!(Period::Week(OCTOBER_22 - 21).label(), "Week of 1 October");
    assert_eq!(Period::Month(2026, 9).label(), "September 2026");
    assert_eq!(Period::Month(1970, 1).label(), "January 1970");
    assert_eq!(Period::Month(2026, 12).label(), "December 2026");
}

/// The folds of the day of `now`, the groups of `changed` folded or
/// opened.
fn folds(now: u64, changed: &[Period]) -> Folds {
    Folds {
        day: day_of(now),
        changed: changed.iter().copied().collect(),
    }
}

/// Snapshots of today, Wednesday, the week of 12 October and September,
/// in the order of a sort by name.
fn snapshots() -> Vec<SnapshotInfo> {
    vec![
        snapshot("a-september", midnight(-40), 1024),
        snapshot("b-today", midnight(0) + HOUR, 2048),
        snapshot("c-week", midnight(-9), 10),
        snapshot("d-wednesday", midnight(-1), 1),
        snapshot("e-today", midnight(0), 4096),
        snapshot("f-september", midnight(-30), 1),
    ]
}

#[test]
fn groups_come_newest_first_their_snapshots_in_the_sort_order() {
    let snapshots = snapshots();
    let now = midnight(0) + 2 * HOUR;
    let opened = folds(now, &[Period::Month(2026, 9)]);
    let (timeline, matches) = Timeline::build(&snapshots, now, &opened);

    let groups: Vec<(Period, usize, u64)> = timeline
        .groups()
        .iter()
        .map(|group| (group.period, group.count, group.used))
        .collect();
    assert_eq!(
        groups,
        [
            (Period::Today, 2, 6144),
            (Period::Day(OCTOBER_22 - 1), 1, 1),
            (Period::Week(OCTOBER_22 - 10), 1, 10),
            (Period::Month(2026, 9), 2, 1025),
        ]
    );
    let order: Vec<usize> = matches.iter().map(|ranked| ranked.index).collect();
    assert_eq!(order, [1, 4, 3, 2, 0, 5]);
    assert_eq!(timeline.rows(), 10);
    // The headers are no stops: the stops are drawn below them.
    let rows: Vec<usize> = (0..6).map(|position| timeline.row_of(position)).collect();
    assert_eq!(rows, [1, 2, 4, 6, 8, 9]);
    assert_eq!(timeline.headers(), [0, 3, 5, 7]);
    assert_eq!(timeline.groups()[0].facts(), ["2 snapshots", "6.0 KiB"]);
    assert_eq!(timeline.groups()[1].facts(), ["1 snapshot", "1 B"]);
}

#[test]
fn a_folded_group_is_its_header_alone_and_one_stop() {
    let snapshots = snapshots();
    let now = midnight(0) + 2 * HOUR;
    let changed = [
        Period::Today,
        Period::Week(OCTOBER_22 - 10),
        Period::Month(2026, 9),
    ];
    let (timeline, matches) = Timeline::build(&snapshots, now, &folds(now, &changed));

    // The first of each folded group stands for it.
    let order: Vec<usize> = matches.iter().map(|ranked| ranked.index).collect();
    assert_eq!(order, [1, 3, 2, 0, 5]);
    assert_eq!(timeline.rows(), 7);
    let rows: Vec<usize> = (0..5).map(|position| timeline.row_of(position)).collect();
    assert_eq!(rows, [0, 2, 3, 5, 6]);
    assert_eq!(timeline.headers(), [1, 4]);
    assert!(timeline.group_at(0).is_some_and(|group| group.folded));
    assert!(timeline.group_at(1).is_some_and(|group| !group.folded));
    assert_eq!(timeline.group_at(5), None);
    assert_eq!(
        timeline.groups()[0].facts(),
        ["2 snapshots", "6.0 KiB", "folded"]
    );
}

#[test]
fn the_rows_of_a_window_are_built_from_its_first_alone() {
    let snapshots = snapshots();
    let now = midnight(0) + 2 * HOUR;
    let opened = folds(now, &[Period::Month(2026, 9)]);
    let (timeline, _) = Timeline::build(&snapshots, now, &opened);
    let groups = timeline.groups();

    assert_eq!(
        timeline.window(2, 4),
        [
            Row::Snapshot(1),
            Row::Header(&groups[1]),
            Row::Snapshot(2),
            Row::Header(&groups[2]),
        ]
    );
    assert_eq!(timeline.window(8, 10), [Row::Snapshot(4), Row::Snapshot(5)]);
    assert_eq!(timeline.window(10, 3), []);
    assert_eq!(timeline.window(0, 0), []);
    assert_eq!(Timeline::default().window(0, 5), []);
    assert_eq!(Timeline::default().rows(), 0);
}

#[test]
fn ranking_selects_a_snapshot_by_name_or_its_folded_group() {
    let snapshots = snapshots();
    let now = midnight(0) + 2 * HOUR;
    let mut list = ListState::default();

    let mut opened = folds(now, &[Period::Month(2026, 9)]);
    rank(&mut list, &snapshots, now, &mut opened, Some("d-wednesday"));
    assert_eq!(list.selected, 2);
    let mut folded = folds(now, &[Period::Day(OCTOBER_22 - 1)]);
    rank(&mut list, &snapshots, now, &mut folded, Some("d-wednesday"));
    assert_eq!(list.selected, 2);
    // The month is folded: its header stands for its snapshots.
    rank(&mut list, &snapshots, now, &mut folded, Some("f-september"));
    assert_eq!(list.selected, 4);
    // A name that is gone keeps the selection in range.
    list.selected = 9;
    rank(&mut list, &snapshots, now, &mut folded, Some("gone"));
    assert_eq!(list.selected, 4);
}

#[test]
fn the_months_are_folded_until_opened_and_the_next_day_forgets_it() {
    let snapshots = snapshots();
    let now = midnight(0) + 2 * HOUR;
    let mut folds = Folds::default();
    folds.on(now);

    let (timeline, _) = Timeline::build(&snapshots, now, &folds);
    let folded: Vec<bool> = timeline.groups().iter().map(|g| g.folded).collect();
    assert_eq!(folded, [false, false, false, true]);
    folds.toggle(Period::Month(2026, 9));
    folds.toggle(Period::Today);
    let (timeline, _) = Timeline::build(&snapshots, now, &folds);
    let folded: Vec<bool> = timeline.groups().iter().map(|g| g.folded).collect();
    assert_eq!(folded, [true, false, false, false]);
    folds.toggle(Period::Today);
    assert!(!folds.is_folded(Period::Today));

    // Later the same day nothing changes; the next day, what was folded
    // or opened is forgotten, as the groups changed: today's is
    // yesterday's.
    folds.toggle(Period::Today);
    folds.on(now + 20 * HOUR);
    assert!(folds.is_folded(Period::Today));
    folds.on(now + 22 * HOUR);
    assert!(!folds.is_folded(Period::Today));
    assert!(folds.is_folded(Period::Month(2026, 9)));
}
