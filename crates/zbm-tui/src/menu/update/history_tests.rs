//! The boot history (ADR 0016, point 7), which the workspace alone asks
//! for and shows: never while the countdown runs, once the main view is
//! up, and again from the Log tab when it did not come. Its text is the
//! ESP's, drawn printable.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};
use zbm_types::{BootRecord, Untrusted};

use super::{Message, events, update};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine};
use crate::menu::keymap::Map;
use crate::menu::model::{Model, Screen};
use crate::menu::theme::Theme;
use crate::menu::view::{tests::screen, words};
use crate::menu::world::{Action, Event, History, Offer, Outcome, Recorded, SavedBoot, Snapshot};

/// The demo machine at the end of start-up, offering everything.
fn machine() -> Snapshot {
    let demo = FakeMachine::new(&DemoOptions::default());
    let mut snapshot = demo.snapshot().clone();
    for stage in &mut snapshot.startup {
        stage.state = crate::menu::world::StageState::Done;
    }
    snapshot.offers = Offer::ALL.into_iter().collect();
    snapshot
}

/// The menu's main view with the key map `map`, its countdown running.
fn menu(map: Map) -> Model {
    let mut model = Model::new(machine(), Theme::new(Capabilities::FULL));
    model.map = map;
    model.refresh();
    events::start(&mut model);
    model.countdown = Some(10_000);
    model
}

fn key(c: char) -> Message {
    Message::Key(KeyPress::new(Key::Char(c)))
}

fn loaded(history: History) -> Message {
    Message::Event(Box::new(Event::HistoryLoaded(history)))
}

/// A saved boot named `name`, with `record`.
fn boot(name: &str, record: Recorded) -> SavedBoot {
    SavedBoot {
        name: Untrusted::new(name.to_owned()),
        files: Vec::new(),
        record,
    }
}

/// A record of `zroot/ROOT/void` with `kernel`, at 2026-10-22 19:00.
fn record(kernel: &str) -> BootRecord {
    BootRecord {
        time: 1_792_695_600,
        environment: Untrusted::new("zroot/ROOT/void".to_owned()),
        kernel: Untrusted::new(kernel.to_owned()),
        measured: true,
        ..BootRecord::default()
    }
}

#[test]
fn the_workspace_asks_once_the_countdown_stopped_and_the_classic_menu_never() {
    let (model, actions) = update(menu(Map::Workspace), Message::Tick(100));
    assert_eq!(actions, []);
    assert!(!model.reading_history);

    let (model, actions) = update(model, Message::Key(KeyPress::new(Key::Escape)));
    assert_eq!(actions, []);
    let (model, actions) = update(model, Message::Tick(100));
    assert_eq!(actions, [Action::ScanPools, Action::LoadHistory]);
    assert!(model.reading_history);
    // Asked, not answered: the Log tab does not ask again.
    let (model, actions) = update(model, key('4'));
    assert_eq!(actions, []);
    assert_eq!(model.screen, Screen::Log);

    let mut classic = menu(Map::Classic);
    classic.countdown = None;
    let (classic, actions) = update(classic, Message::Tick(100));
    assert_eq!(actions, [Action::ScanPools]);
    let (classic, actions) = update(classic, key('l'));
    assert_eq!(classic.screen, Screen::Log);
    assert_eq!(actions, []);
}

#[test]
fn the_log_tab_asks_again_for_a_history_that_did_not_come() {
    let mut model = menu(Map::Workspace);
    model.countdown = None;
    model.reading_history = true;
    // Escape cancelled the read: the machine sends no history, only that
    // the read was cancelled.
    let cancelled = Event::Finished(Outcome::Cancelled);
    let (model, _) = update(model, Message::Event(Box::new(cancelled)));
    let (model, _) = update(model, Message::Busy(false));
    assert_eq!(model.history, None);
    assert!(!model.reading_history);

    let (model, actions) = update(model, key('4'));
    assert_eq!(actions, [Action::LoadHistory]);

    let (model, actions) = update(model, loaded(History::Off));
    assert_eq!(actions, []);
    assert_eq!(model.history, Some(History::Off));
    assert!(!model.reading_history);
    let (model, _) = update(model, key('1'));
    let (_, actions) = update(model, key('4'));
    assert_eq!(actions, []);
}

#[test]
fn the_log_tab_says_how_to_keep_a_history_without_save_logs() {
    let mut model = menu(Map::Workspace);
    model.countdown = None;
    model.screen = Screen::Log;
    model.history = Some(History::Off);

    let shown = screen(&model, 80, 24);

    assert!(
        shown.contains("No boot is kept without zbm.save_logs."),
        "{shown}"
    );
    assert!(shown.contains("zfsbootmenu(7)"), "{shown}");
    let (rows, _) = words::rows(&model);
    assert_eq!(
        rows.first().map(String::as_str),
        Some("No boot is kept without zbm.save_logs.")
    );
}

#[test]
fn the_history_draws_the_esp_s_text_printable_newest_first() {
    let mut model = menu(Map::Workspace);
    model.countdown = None;
    model.screen = Screen::Log;
    model.history = Some(History::Saved(vec![
        boot(
            "2026-10-22T19-00-00Z",
            Recorded::Read(record("/boot/vmlinuz-6.12.9\u{1b}]0;owned\u{7}")),
        ),
        boot("2026-10-22T08-00-00Z\u{1b}[2J", Recorded::Missing),
        boot(
            "2026-10-21T08-00-00Z",
            Recorded::Unreadable("the record has schema 2, not 1".to_owned()),
        ),
    ]));

    let shown = screen(&model, 120, 40);

    assert!(!shown.contains('\u{1b}'), "{shown}");
    assert!(shown.contains("6.12.9\\x1b]0;owned\\x07"), "{shown}");
    assert!(
        shown.contains("2026-10-22 19:00  zroot/ROOT/void"),
        "{shown}"
    );
    assert!(shown.contains("2026-10-22T08-00-00Z\\x1b[2J  saved without a record"));
    assert!(shown.contains("its record cannot be read: the record has schema 2"));
    let (rows, _) = words::rows(&model);
    assert!(
        rows.iter().take(3).all(|row| !row.contains('\u{1b}')),
        "{rows:?}"
    );
    // The classic menu's log shows nothing of it.
    model.map = Map::Classic;
    assert!(!screen(&model, 120, 40).contains("SAVED BOOTS"));
    assert!(
        words::rows(&model)
            .0
            .iter()
            .all(|row| !row.contains("zroot/ROOT/void"))
    );
}

#[test]
fn an_unreadable_esp_says_why_with_its_code() {
    let mut model = menu(Map::Workspace);
    model.countdown = None;
    model.screen = Screen::Log;
    model.history = Some(History::Unavailable {
        why: "unable to mount /dev/vda1".to_owned(),
        code: 624,
    });

    let shown = screen(&model, 120, 40);

    assert!(
        shown.contains("The saved boots cannot be read: unable to mount /dev/vda1 (ZBM-E0624)."),
        "{shown}"
    );
    model.history = Some(History::Saved(Vec::new()));
    assert!(screen(&model, 120, 40).contains("No boot is saved yet."));
    model.history = None;
    assert!(screen(&model, 120, 40).contains("Reading the saved boots..."));
}

#[test]
fn the_inspector_says_when_the_environment_was_last_loaded() {
    let mut model = menu(Map::Workspace);
    model.countdown = None;
    model.history = Some(History::Saved(vec![
        boot("2026-10-23T07-00-00Z", Recorded::Missing),
        boot(
            "2026-10-22T19-00-00Z",
            Recorded::Read(record("/boot/vmlinuz-6.12.9_1")),
        ),
        boot(
            "2026-10-21T19-00-00Z",
            Recorded::Read(record("/boot/vmlinuz-6.12.8_1")),
        ),
    ]));

    // Wide enough for the fact not to wrap.
    let shown = screen(&model, 200, 40);

    assert!(
        shown.contains("Last Boot  Loaded 2026-10-22 19:00 with 6.12.9_1"),
        "{shown}"
    );
    model.map = Map::Classic;
    assert!(!screen(&model, 160, 40).contains("Last Boot"));
}
