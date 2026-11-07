//! Tests of the workspace's Snapshots tab as a timeline (ADR 0016, point
//! 9): the selection goes from snapshot to snapshot over the headers of
//! the groups, which the line view does not number; the months are
//! folded; a group folds and unfolds around the selection, and stays
//! folded, until midnight, when the clock moves the groups; on a folded
//! group, the keys of a snapshot's actions are not shown and say to open
//! it; `o` sorts within the groups; a filter ranks the snapshots without
//! groups; a refresh keeps the snapshot selected by name; and the actions
//! on a snapshot are offered and performed as in the current menu, whose
//! map gains no key.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::{Message, update};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::keymap::{Context, Map};
use crate::menu::lines::{self, Typed};
use crate::menu::model::{Dialog, Model, Screen};
use crate::menu::theme::Theme;
use crate::menu::view::words;
use crate::menu::world::{Action, Event, Outcome, SnapshotInfo, SnapshotList};
use crate::text::text;

/// The demo, started, its countdown stopped, on the Snapshots tab of the
/// default environment, read: in the workspace when `workspace`.
fn tab(workspace: bool) -> Session {
    let options = DemoOptions {
        classic: !workspace,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char(if workspace { '2' } else { 's' }));
    session
}

/// The name of the selected snapshot, after its `@`; empty on the header
/// of a folded group.
fn selected(model: &Model) -> String {
    model
        .selected_snapshot()
        .map(|snapshot| snapshot.name.name().to_owned())
        .unwrap_or_default()
}

/// The daily snapshot of the demo taken on `date` of October 2026.
fn daily(date: u32) -> String {
    format!("autosnap_2026-10-{date:02}_daily")
}

/// `session` after `key` pressed `times` times.
fn press(session: &mut Session, key: Key, times: usize) {
    for _ in 0..times {
        session.key(key);
    }
}

#[test]
fn up_and_down_go_from_snapshot_to_snapshot_over_the_headers() {
    let mut session = tab(true);
    assert_eq!(selected(session.model()), "pre-upgrade");

    let mut seen = Vec::new();
    for _ in 0..5 {
        session.key(Key::Down);
        seen.push(selected(session.model()));
    }
    assert_eq!(
        seen,
        [daily(22), daily(21), daily(20), daily(19), daily(12)]
    );
    session.key(Key::Up);
    assert_eq!(selected(session.model()), daily(19));
    session.key(Key::Home);
    session.key(Key::PageDown);
    assert_eq!(selected(session.model()), daily(17));
    // The months are folded: each is one stop, the oldest the last.
    session.key(Key::End);
    let label = |model: &Model| model.folded_group().map(|group| group.period.label());
    assert_eq!(label(session.model()).as_deref(), Some("September 2025"));
    assert_eq!(session.model().snapshots.selected, 35);
    session.key(Key::Up);
    assert_eq!(label(session.model()).as_deref(), Some("October 2025"));
    session.key(Key::PageUp);
    assert_eq!(label(session.model()).as_deref(), Some("August 2026"));
    session.key(Key::PageUp);
    assert_eq!(selected(session.model()), daily(7));
}

#[test]
fn the_line_view_numbers_the_snapshots_and_not_the_headers() {
    let mut session = tab(true);

    let page = lines::page(session.model());
    let at = |line: &str| page.iter().position(|said| said == line);
    let today = at("  Today, 1 snapshot, 1.1 GiB").unwrap();
    assert_eq!(page[today + 1], "  1) pre-upgrade, 1 h ago, 1.1 GiB");
    assert_eq!(page[today + 2], "  Thursday 22 October, 1 snapshot, 17 MiB");
    assert_eq!(
        page[today + 3],
        format!("  2) {}, 1 d ago, 17 MiB", daily(22))
    );
    assert!(at("  Week of 12 October, 7 snapshots, 301 MiB").is_some());
    // The week of 28 September is that of 1 October in October.
    assert!(at("  Week of 1 October, 4 snapshots, 172 MiB").is_some());
    // A folded month is numbered, as it is a stop.
    assert!(
        at("  24) September 2026, 30 snapshots, 1.2 GiB, folded").is_some(),
        "{page:#?}"
    );
    let numbered = page.iter().filter(|said| said.contains(") ")).count();
    assert_eq!(numbered, 36);

    // A number chooses the snapshot it numbers.
    let Typed::Keys(keys) = lines::typed("3 r", Context::Snapshots, Map::Workspace) else {
        panic!("not keys");
    };
    for key in keys {
        session.press(key);
    }
    assert_eq!(selected(session.model()), daily(21));
    let Some(Dialog::Review(reviewing)) = &session.model().dialog else {
        panic!("{:?}", session.model().dialog);
    };
    assert!(
        matches!(&reviewing.action, Action::Rollback(name) if name.name() == daily(21)),
        "{:?}",
        reviewing.action
    );
}

#[test]
fn a_group_folds_and_unfolds_around_the_selection_and_stays_folded() {
    let mut session = tab(true);
    press(&mut session, Key::Down, 7);
    assert_eq!(selected(session.model()), daily(14));

    session.key(Key::Char('z'));
    let model = session.model();
    let group = model.folded_group().map(|group| group.period.label());
    assert_eq!(group.as_deref(), Some("Week of 12 October"));
    assert_eq!(selected(model), "");
    assert_eq!(model.path(), ["zroot", "zroot/ROOT/void"]);
    let page = lines::page(model);
    assert!(
        page.contains(&"  6) Week of 12 October, 7 snapshots, 301 MiB, folded".to_owned()),
        "{page:#?}"
    );
    // Its snapshots are no stops: one step goes over them.
    session.key(Key::Down);
    assert_eq!(selected(session.model()), daily(5));
    session.key(Key::Up);
    assert_eq!(selected(session.model()), "");
    // Nothing is done to a folded group but opening it: the keys of a
    // snapshot's actions are not shown, and say so.
    let commands = words::commands(session.model());
    assert!(
        commands.contains(&"z fold or unfold".to_owned()),
        "{commands:?}"
    );
    assert!(
        !commands.iter().any(|command| command.starts_with("r ")),
        "{commands:?}"
    );
    session.key(Key::Char('r'));
    let model = session.model();
    assert_eq!(model.dialog, None);
    let said = Outcome::Warning(text!("timeline-open-group"));
    assert_eq!(model.notification, Some(said));

    // Enter opens it, on its first snapshot; `z` folds it again.
    session.key(Key::Enter);
    assert_eq!(selected(session.model()), daily(12));
    assert_eq!(session.model().folded_group(), None);
    session.key(Key::Char('z'));
    assert!(session.model().folded_group().is_some());

    // The machine's next snapshot keeps it folded and selected; so does
    // the Boot tab, and the snapshots of another environment.
    let model = session.into_model();
    let snapshot = model.snapshot.clone();
    let (model, _) = update(
        model,
        Message::Event(Box::new(Event::Changed(Box::new(snapshot)))),
    );
    assert!(model.folded_group().is_some());
    let options = DemoOptions {
        classic: false,
        ..DemoOptions::default()
    };
    let mut session = Session::with_model(crate::menu::demo::FakeMachine::new(&options), model);
    session.key(Key::Char('1'));
    session.key(Key::Char('2'));
    assert!(session.model().folded_group().is_some());
    session.key(Key::Char('z'));
    assert_eq!(selected(session.model()), daily(12));
}

#[test]
fn sorting_orders_each_group_and_the_groups_stay_newest_first() {
    let mut session = tab(true);
    press(&mut session, Key::Down, 5);
    assert_eq!(selected(session.model()), daily(12));

    // By creation, then by size: the selection stays on its snapshot.
    session.key(Key::Char('o'));
    assert_eq!(selected(session.model()), daily(12));
    session.key(Key::Home);
    press(&mut session, Key::Down, 5);
    assert_eq!(selected(session.model()), daily(18));
    let page = lines::page(session.model());
    let headers: Vec<&String> = page
        .iter()
        .filter(|said| said.starts_with("  ") && !said.contains(") "))
        .take(4)
        .collect();
    assert_eq!(
        headers,
        [
            "  Today, 1 snapshot, 1.1 GiB",
            "  Thursday 22 October, 1 snapshot, 17 MiB",
            "  Wednesday 21 October, 1 snapshot, 30 MiB",
            "  Tuesday 20 October, 1 snapshot, 43 MiB",
        ]
    );
    session.key(Key::Char('o'));
    // The largest of the week of 12 October: 82 MiB, taken on the 17th.
    assert_eq!(selected(session.model()), daily(18));
    session.key(Key::Home);
    press(&mut session, Key::Down, 5);
    assert_eq!(selected(session.model()), daily(17));
}

#[test]
fn a_filter_ranks_the_snapshots_without_groups() {
    let mut session = tab(true);
    session.key(Key::Char('/'));
    session.type_text("10-1");

    let model = session.model();
    assert_eq!(model.timeline(), None);
    let (rows, numbered) = words::rows(model);
    assert!(numbered);
    assert_eq!(words::headers(model), Vec::<usize>::new());
    assert!(
        rows.iter().all(|row| row.starts_with("autosnap_")),
        "{rows:#?}"
    );
    assert!(
        rows.iter().any(|row| row.starts_with(&daily(19))),
        "{rows:#?}"
    );

    // Esc clears it, and the groups come back.
    session.key(Key::Escape);
    assert!(session.model().timeline().is_some());
}

/// The snapshots of the default environment the menu read.
fn loaded(model: &Model) -> Vec<SnapshotInfo> {
    model.focus_snapshots().to_vec()
}

#[test]
fn a_refresh_keeps_the_snapshot_selected_by_name() {
    let mut session = tab(true);
    press(&mut session, Key::Down, 7);
    let model = session.into_model();
    let mut snapshots = loaded(&model);
    let newest = SnapshotInfo {
        name: "zroot/ROOT/void@zzz-newest".parse().unwrap(),
        created: model.snapshot.now,
        used: 1,
    };
    snapshots.push(newest);
    let list = SnapshotList {
        environment: "zroot/ROOT/void".parse().unwrap(),
        snapshots: snapshots.clone(),
    };

    let (model, _) = update(
        model,
        Message::Event(Box::new(Event::SnapshotsLoaded(list))),
    );
    assert_eq!(selected(&model), daily(14));
    assert_eq!(model.snapshots.selected, 8);

    // Once it is gone, the selection stays where it was.
    snapshots.retain(|snapshot| snapshot.name.name() != daily(14));
    let list = SnapshotList {
        environment: "zroot/ROOT/void".parse().unwrap(),
        snapshots,
    };
    let (model, _) = update(
        model,
        Message::Event(Box::new(Event::SnapshotsLoaded(list))),
    );
    assert_eq!(model.snapshots.selected, 8);
    assert_eq!(selected(&model), daily(15));
}

#[test]
fn the_actions_on_a_snapshot_start_from_the_timeline() {
    let mut session = tab(true);
    press(&mut session, Key::Down, 7);

    session.key(Key::Char('r'));
    let Some(Dialog::Review(reviewing)) = &session.model().dialog else {
        panic!("{:?}", session.model().dialog);
    };
    assert_eq!(
        reviewing.action,
        Action::Rollback(format!("zroot/ROOT/void@{}", daily(14)).parse().unwrap())
    );
    // The demo planned it, and the page asks for the environment's name.
    assert!(reviewing.plan.is_some());
    assert_eq!(reviewing.name, "void");
    session.key(Key::Escape);
    session.key(Key::Enter);
    assert!(
        matches!(&session.model().dialog, Some(Dialog::Choose { .. })),
        "{:?}",
        session.model().dialog
    );
    session.key(Key::Escape);
    session.key(Key::Char(' '));
    assert_eq!(session.model().marked.len(), 1);
    assert_eq!(session.model().screen, Screen::Snapshots);
}

#[test]
fn the_current_map_lists_the_snapshots_without_groups_and_no_key_folds() {
    let mut session = tab(false);
    assert_eq!(session.model().timeline(), None);
    session.key(Key::Down);
    let before = session.model().snapshots.selected;

    session.press(KeyPress::new(Key::Char('z')));

    assert_eq!(session.model().snapshots.selected, before);
    assert_eq!(session.model().timeline(), None);
    let (rows, numbered) = words::rows(session.model());
    assert!(numbered && rows.len() == 400);
}

#[test]
fn a_diff_of_marked_snapshots_starts_from_a_folded_group() {
    let mut session = tab(true);
    session.key(Key::Char(' '));
    session.key(Key::Down);
    session.key(Key::Char(' '));
    press(&mut session, Key::Down, 6);
    session.key(Key::Char('z'));
    assert!(session.model().folded_group().is_some());
    let commands = words::commands(session.model());
    assert!(
        commands.iter().any(|command| command.starts_with("f ")),
        "{commands:?}"
    );

    let model = session.into_model();
    let (_, actions) = update(model, Message::Key(KeyPress::new(Key::Char('f'))));
    let name = |date: &str| format!("zroot/ROOT/void@{date}").parse().unwrap();
    // The older first, whichever was marked first.
    assert_eq!(
        actions,
        [Action::Diff(name(&daily(22)), Some(name("pre-upgrade")))]
    );
}

#[test]
fn at_midnight_the_clock_moves_the_groups_and_forgets_the_folds() {
    let mut session = tab(true);
    press(&mut session, Key::Down, 7);
    session.key(Key::Char('z'));
    assert!(session.model().folded_group().is_some());

    // 08:00 on Friday 23 October: sixteen hours to midnight.
    session.tick(16 * 3600 * 1000 - 1000);
    let page = lines::page(session.model());
    assert!(page.contains(&"  Today, 1 snapshot, 1.1 GiB".to_owned()));
    session.tick(1000);
    let model = session.model();
    let page = lines::page(model);
    assert!(!page.iter().any(|said| said.starts_with("  Today")));
    assert!(
        page.contains(&"  Friday 23 October, 1 snapshot, 1.1 GiB".to_owned()),
        "{page:#?}"
    );
    // The group open again, its first snapshot stays selected, which
    // stood for it.
    assert_eq!(model.folded_group(), None);
    assert_eq!(selected(model), daily(12));
}
