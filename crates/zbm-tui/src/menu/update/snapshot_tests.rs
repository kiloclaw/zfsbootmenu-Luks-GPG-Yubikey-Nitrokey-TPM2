//! The snapshots on demand (ADR 0016, point 6): a column asks for its
//! snapshots as it opens, says so until they come, and says they are not
//! read after a cancel, when the next snapshot of the machine fills it, or
//! after a failure, when opening it again does. What the menu knows of
//! snapshots is what it read.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::{Message, events, update};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine, Session};
use crate::menu::model::{Column, Dialog, Model, Screen, Sort, Then};
use crate::menu::theme::Theme;
use crate::menu::view::words;
use crate::menu::world::{Action, Event, Offer, Outcome, Snapshot, SnapshotList};

/// The demo in the classic menu (`zbm.menu=classic`), whose keys these
/// tests press.
fn classic() -> DemoOptions {
    DemoOptions {
        classic: true,
        ..DemoOptions::default()
    }
}

/// The demo machine, offering everything: its snapshot, no column read,
/// and the snapshots of the default environment, which a load sends.
fn machine() -> (Snapshot, SnapshotList) {
    let mut demo = FakeMachine::new(&classic());
    let mut snapshot = demo.snapshot().clone();
    for stage in &mut snapshot.startup {
        stage.state = crate::menu::world::StageState::Done;
    }
    snapshot.offers = Offer::ALL.into_iter().collect();
    let events = demo.apply(Action::LoadSnapshots(void()));
    let Some(Event::SnapshotsLoaded(list)) = events.into_iter().next() else {
        panic!("no snapshots");
    };
    (snapshot, list)
}

fn void() -> zbm_types::DatasetName {
    "zroot/ROOT/void".parse().unwrap()
}

/// The classic menu's main view on `snapshot`, the countdown stopped.
fn menu(snapshot: Snapshot) -> Model {
    let mut model = Model::new(snapshot, Theme::new(Capabilities::FULL));
    model.map = crate::menu::keymap::Map::Classic;
    model.refresh();
    events::start(&mut model);
    model.countdown = None;
    model
}

/// `model` after `message`, and the actions it asks for.
fn step(model: Model, message: Message) -> (Model, Vec<Action>) {
    update(model, message)
}

fn key(c: char) -> Message {
    Message::Key(KeyPress::new(Key::Char(c)))
}

fn event(event: Event) -> Message {
    Message::Event(Box::new(event))
}

#[test]
fn a_column_asks_for_its_snapshots_and_says_so_until_they_come() {
    let (snapshot, list) = machine();
    let (model, actions) = step(menu(snapshot), key('s'));

    assert_eq!(actions, [Action::LoadSnapshots(void())]);
    assert_eq!(model.screen, Screen::Snapshots);
    assert_eq!(model.column(), Column::Reading);
    assert!(model.focus_snapshots().is_empty());
    assert_eq!(
        words::rows(&model),
        (
            vec!["Reading the Snapshots...: The machine lists them; Esc stops it.".to_owned()],
            false
        )
    );

    let (model, actions) = step(model, event(Event::SnapshotsLoaded(list)));
    assert!(actions.is_empty());
    assert_eq!(model.column(), Column::Loaded);
    assert_eq!(model.focus_snapshots().len(), 400);
    let (rows, numbered) = words::rows(&model);
    assert!(numbered && rows.len() == 400);
}

#[test]
fn a_cancelled_load_says_not_read_and_the_next_snapshot_asks_again() {
    let (snapshot, list) = machine();
    let (model, _) = step(menu(snapshot.clone()), key('s'));
    let (model, _) = step(model, Message::Busy(true));

    let (model, _) = step(model, event(Event::Finished(Outcome::Cancelled)));
    // The load ends once the machine is done with all it was sent.
    assert_eq!(model.column(), Column::Reading);
    let (model, _) = step(model, Message::Busy(false));
    assert_eq!(model.column(), Column::NotRead);
    let (rows, _) = words::rows(&model);
    assert_eq!(
        rows,
        ["Snapshots Not Read: Open the snapshots again to read them."]
    );

    let (model, actions) = step(model, event(Event::Changed(Box::new(snapshot.clone()))));
    assert_eq!(actions, [Action::LoadSnapshots(void())]);
    assert_eq!(model.column(), Column::Reading);
    // While the load runs, another snapshot does not ask again; one that
    // carries the column fills it.
    let (model, actions) = step(model, event(Event::Changed(Box::new(snapshot.clone()))));
    assert!(actions.is_empty());
    let mut carried = snapshot;
    carried.loaded = vec![list];
    let (model, actions) = step(model, event(Event::Changed(Box::new(carried))));
    assert!(actions.is_empty());
    assert_eq!(model.column(), Column::Loaded);
}

#[test]
fn no_snapshots_are_asked_for_while_the_countdown_runs() {
    let (snapshot, _) = machine();
    let mut model = menu(snapshot.clone());
    model.focus = Some(void());
    model.screen = Screen::Snapshots;
    model.countdown = Some(5_000);

    let (model, actions) = step(model, event(Event::Changed(Box::new(snapshot))));

    assert!(actions.is_empty());
    assert_eq!(model.column(), Column::NotRead);
    assert!(model.countdown.is_some());
}

#[test]
fn the_rollback_counts_the_newer_snapshots_of_the_latest_read() {
    let (snapshot, mut list) = machine();
    let (model, _) = step(menu(snapshot.clone()), key('s'));
    let (mut model, _) = step(model, event(Event::SnapshotsLoaded(list.clone())));
    model.set_sort(Sort::Creation);
    // A change elsewhere destroyed all but the two oldest.
    let oldest = list.snapshots.split_off(list.snapshots.len() - 2);
    let mut carried = snapshot;
    carried.loaded = vec![SnapshotList {
        environment: void(),
        snapshots: oldest,
    }];
    let (mut model, _) = step(model, event(Event::Changed(Box::new(carried))));
    model.snapshots.selected = 1;

    let (model, _) = step(model, key('r'));

    let Some(Dialog::Confirm { lines, .. }) = &model.dialog else {
        panic!("{:?}", model.dialog);
    };
    assert!(lines[1].starts_with("One newer snapshot"), "{lines:?}");
}

#[test]
fn the_rollback_adds_the_sizes_the_pool_gives_up_to_the_largest() {
    let (snapshot, mut list) = machine();
    for taken in &mut list.snapshots {
        taken.used = u64::MAX;
    }
    let (model, _) = step(menu(snapshot), key('s'));
    let (mut model, _) = step(model, event(Event::SnapshotsLoaded(list)));
    model.set_sort(Sort::Creation);
    // The oldest: every other snapshot is newer.
    model.snapshots.selected = model.snapshots.matches.len() - 1;

    let (model, _) = step(model, key('r'));

    let Some(Dialog::Confirm { lines, .. }) = &model.dialog else {
        panic!("{:?}", model.dialog);
    };
    let largest = crate::menu::format::size(u64::MAX);
    assert!(lines[1].contains(&largest), "{lines:?}");
}

#[test]
fn a_taken_name_is_known_only_in_a_column_read_and_zfs_refuses_the_others() {
    let mut session = Session::new(&classic(), Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    // `tank/ROOT/arch` has `2026-10-01`, its column never opened.
    session.key(Key::Down);
    session.key(Key::Char('n'));
    session.press(KeyPress::new(Key::Char('u')).with_ctrl());
    session.type_text("2026-10-01");
    session.key(Key::Enter);

    let Some(Dialog::Error { why, .. }) = &session.model().dialog else {
        panic!("{:?}", session.model().dialog);
    };
    assert_eq!(
        why,
        &["cannot create snapshot 'tank/ROOT/arch@2026-10-01': dataset already exists"]
    );
    session.key(Key::Escape);

    // Once read, the menu knows the name and asks nothing.
    session.key(Key::Char('s'));
    session.key(Key::Char('n'));
    session.press(KeyPress::new(Key::Char('u')).with_ctrl());
    session.type_text("2026-10-01");
    let before = session.actions.len();
    session.key(Key::Enter);
    assert_eq!(session.actions.len(), before);
    assert!(matches!(
        &session.model().notification,
        Some(Outcome::Warning(said)) if said.contains("exists already")
    ));
}

#[test]
fn the_palette_open_finds_the_snapshots_that_come() {
    let (snapshot, list) = machine();
    let (model, _) = step(menu(snapshot), key(':'));
    let count = model.palette.as_ref().map(|p| p.items.len()).unwrap();

    let (model, _) = step(model, event(Event::SnapshotsLoaded(list)));

    let palette = model.palette.as_ref().unwrap();
    assert_eq!(palette.items.len(), count + 400);
    assert_eq!(palette.matches.len(), palette.items.len());
    assert!(matches!(&palette.items[4].then, Then::Name(_)));
}

#[test]
fn another_action_that_fails_leaves_the_load_queued_behind_it_reading() {
    let (snapshot, list) = machine();
    let model = menu(snapshot);
    let (model, _) = step(model, Message::Busy(true));
    let (model, _) = step(model, key('s'));

    let failed = Outcome::Failed {
        what: "unable to create the snapshot".to_owned(),
        why: Vec::new(),
        next: Vec::new(),
    };
    let (model, _) = step(model, event(Event::Finished(failed)));

    assert_eq!(model.column(), Column::Reading);
    let (model, _) = step(model, event(Event::SnapshotsLoaded(list)));
    let (model, _) = step(model, Message::Busy(false));
    assert_eq!(model.column(), Column::Loaded);
}

#[test]
fn a_load_that_failed_is_asked_again_only_by_opening_its_column() {
    let (snapshot, list) = machine();
    let mut model = menu(snapshot.clone());
    model.map = crate::menu::keymap::Map::Workspace;
    let (model, _) = step(model, key('2'));
    let (model, _) = step(model, Message::Busy(true));

    let (model, _) = step(model, event(Event::SnapshotsUnread(void())));
    assert_eq!(model.column(), Column::NotRead);
    let (model, actions) = step(model, event(Event::Changed(Box::new(snapshot.clone()))));
    assert!(actions.is_empty(), "{actions:?}");
    let (model, _) = step(model, Message::Busy(false));

    // On its tab, the tab's key reads them again.
    let (model, actions) = step(model, key('2'));
    assert_eq!(actions, [Action::LoadSnapshots(void())]);
    assert_eq!(model.column(), Column::Reading);
    let (model, _) = step(model, event(Event::SnapshotsLoaded(list)));
    assert_eq!(model.column(), Column::Loaded);
    assert_eq!(model.unread, None);
}

#[test]
fn the_count_of_a_column_not_read_is_the_one_the_machine_read() {
    let (snapshot, _) = machine();
    let (model, _) = step(menu(snapshot), key('s'));

    let screen = crate::menu::view::tests::screen(&model, 120, 40);

    assert_eq!(model.column(), Column::Reading);
    assert!(screen.contains(" 400 ┐"), "{screen}");
}

#[test]
fn an_export_from_the_snapshots_view_asks_for_no_snapshots_of_what_left() {
    let (snapshot, list) = machine();
    let (model, _) = step(menu(snapshot), key('s'));
    let (model, _) = step(model, event(Event::SnapshotsLoaded(list)));
    let mut demo = FakeMachine::new(&classic());
    demo.apply(Action::LoadSnapshots(void()));
    let events = demo.apply(Action::Export(void().pool()));
    let Some(Event::Changed(exported)) = events.into_iter().next() else {
        panic!("no snapshot");
    };
    assert!(exported.loaded.is_empty());

    let (model, actions) = step(model, event(Event::Changed(exported)));

    assert!(actions.is_empty(), "{actions:?}");
    assert!(model.focus_snapshots().is_empty());
    assert!(model.dialog.is_none());
    // Nor does the demo, asked all the same, show what it had.
    let events = demo.apply(Action::LoadSnapshots(void()));
    assert!(matches!(
        events.as_slice(),
        [Event::SnapshotsUnread(gone), Event::Finished(Outcome::Failed { .. })] if *gone == void()
    ));
}
