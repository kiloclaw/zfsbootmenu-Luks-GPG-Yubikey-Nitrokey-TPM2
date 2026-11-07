//! Start-up as the workspace's Boot tab: the keys read between questions,
//! the tabs looked at meanwhile, and the countdown the menu then arms or
//! not; and the current map's view, which reads no key between questions.

use std::cell::{Cell, RefCell};
use std::os::fd::AsFd as _;

use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use zbm_display::keys::Key;

use super::tests::{Scripted, shown, unlock_requested, view_with};
use super::*;
use crate::menu::demo::{DemoOptions, FakeMachine};
use crate::menu::keymap::{Context, Tab};
use crate::menu::model::Screen;
use crate::menu::world::{Outcome, StageState, Timeout};
use crate::text::text;

type View = StartupView<TestBackend, Scripted>;

/// The view of the workspace on `keys`.
fn workspace(keys: &Scripted) -> View {
    view_with(keys, Map::Workspace)
}

/// The stages of `view` end as `state` says, the first done.
fn stages_end(view: &mut View, state: StageState) {
    let ends = [
        ("Unlocking LUKS volumes", StageState::Done),
        ("Importing pools", state),
    ];
    for (stage, state) in ends {
        let finished = Event::StageFinished {
            stage: stage.into(),
            state,
        };
        view.send(finished).unwrap();
    }
}

/// The menu after start-up: the model of `view` in the round the menu
/// runs, once the machine sent its first snapshot, with environments and
/// a countdown of ten seconds.
fn menu(view: &mut View) -> Model {
    let mut model = view.model.take().unwrap();
    model.begin_round();
    let mut snapshot = FakeMachine::new(&DemoOptions::default()).snapshot().clone();
    snapshot.startup.clear();
    snapshot.timeout = Timeout::Seconds(10);
    let changed = Event::Changed(Box::new(snapshot));
    update(model, Message::Event(Box::new(changed))).0
}

/// A key press of the menu on `model`.
fn press(model: Model, code: Key) -> Model {
    let key = crate::menu::keymap::press(code, crate::menu::keymap::Modifiers::NONE);
    update(model, Message::Key(key)).0
}

/// Serves `view` until `running` was asked `calls` times, when the stages
/// close it: the menu's screen.
fn served(view: View, calls: usize) -> SessionScreen<TestBackend, Scripted> {
    let mailbox = Mailbox::new().unwrap();
    let sender = mailbox.sender();
    let (done, closed) = zbm_platform::event::reply();
    let done = RefCell::new(Some(done));
    let asked = Cell::new(0);
    let running = || {
        asked.set(asked.get() + 1);
        if asked.get() == calls
            && let Some(done) = done.borrow_mut().take()
        {
            sender.send(ToView::Close { done }).unwrap();
        }
        true
    };
    let screen = view.serve(mailbox, &running).unwrap().unwrap();
    assert!(closed.wait(None).is_ok());
    screen
}

#[test]
fn the_workspace_reads_keys_between_questions_and_a_tab_change_is_presence() {
    let keys = Scripted::default();
    keys.typed("4");
    let view = workspace(&keys);

    let screen = served(view, 2);

    let model = screen.model().unwrap();
    assert_eq!(
        (model.screen, model.away, model.keys),
        (Screen::Log, Some(Tab::Log), 1)
    );
    assert!(keys.keys.borrow().is_empty());
}

#[test]
fn noise_on_the_line_between_questions_changes_nothing() {
    let keys = Scripted::default();
    // Reads that end at once with no key, one more than the loop's turns.
    keys.noise(4);
    let view = workspace(&keys);

    let screen = served(view, 3);

    let model = screen.model().unwrap();
    assert_eq!(
        (model.screen, model.away, model.keys, model.ticked),
        (Screen::Startup, None, 0, 0)
    );
    assert_eq!(model.notification, None);
    assert!(keys.keys.borrow().is_empty());
}

#[test]
fn the_classic_view_reads_no_key_between_questions() {
    let keys = Scripted::default();
    keys.typed("4l");
    let view = view_with(&keys, Map::Classic);

    let screen = served(view, 2);

    // The keys wait for the menu, whose countdown the first one stops.
    let model = screen.model().unwrap();
    assert_eq!(
        (model.screen, model.away, model.keys),
        (Screen::Startup, None, 0)
    );
    assert_eq!(keys.keys.borrow().len(), 2);
    assert_eq!(*keys.modes.borrow(), ["release"]);
}

#[test]
fn a_passphrase_typed_during_start_up_leaves_the_countdown_armed() {
    let keys = Scripted::default();
    // A digit in a dialog types: it chooses no tab.
    keys.typed("hunter4");
    keys.press(Key::Enter);
    let mut view = workspace(&keys);

    let answer = view.ask(&unlock_requested(None), &mut || None).unwrap();
    assert!(matches!(answer, Action::Unlock { .. }));
    assert_eq!(
        (view.model().screen, view.model().away),
        (Screen::Startup, None)
    );
    stages_end(&mut view, StageState::Done);
    let model = menu(&mut view);

    assert_eq!(model.screen, Screen::Main);
    assert_eq!(model.countdown_seconds(), Some(10));
}

/// Reads `count` keys of `view` between questions.
fn read_keys(view: &mut View, count: usize) {
    let mailbox = Mailbox::<ToView>::new().unwrap();
    let mut last_tick = view.frontend.now();
    for _ in 0..count {
        view.read_key(mailbox.as_fd(), &mut last_tick).unwrap();
    }
}

#[test]
fn a_passphrase_typed_before_its_dialog_waits_for_it_and_the_countdown() {
    let keys = Scripted::default();
    // A digit after a key the Boot tab does not bind types too.
    keys.typed("pass3word");
    keys.press(Key::Enter);
    let mut view = workspace(&keys);

    read_keys(&mut view, 10);
    assert_eq!(
        (view.model().screen, view.model().away, view.model().keys),
        (Screen::Startup, None, 0)
    );
    let answer = view.ask(&unlock_requested(None), &mut || None).unwrap();

    let Action::Unlock { secret, .. } = answer else {
        panic!("no secret: {answer:?}");
    };
    assert_eq!(secret.unwrap().expose(), b"pass3word");
    assert!(view.typed_ahead.is_empty());
    stages_end(&mut view, StageState::Done);
    let model = menu(&mut view);
    assert_eq!(model.screen, Screen::Main);
    assert_eq!(model.countdown_seconds(), Some(10));
}

#[test]
fn keys_typed_ahead_of_no_dialog_go_nowhere() {
    let keys = Scripted::default();
    keys.typed("x2");
    let view = workspace(&keys);

    let screen = served(view, 3);

    let model = screen.model().unwrap();
    assert_eq!(
        (model.screen, model.away, model.keys),
        (Screen::Startup, None, 0)
    );
    assert_eq!(model.notification, None);
    assert!(keys.keys.borrow().is_empty());
}

#[test]
fn a_key_another_tab_does_not_bind_is_not_typed_ahead() {
    let keys = Scripted::default();
    keys.typed("4q1");
    let mut view = workspace(&keys);

    read_keys(&mut view, 3);

    assert!(view.typed_ahead.is_empty());
    assert_eq!(
        (view.model().screen, view.model().away),
        (Screen::Startup, Some(Tab::Log))
    );
}

#[test]
fn the_snapshots_tab_waits_for_start_up_and_tab_passes_it_over() {
    let keys = Scripted::default();
    keys.typed("2");
    keys.press(Key::Tab);
    keys.press(Key::BackTab);
    keys.press(Key::BackTab);
    keys.press(Key::Tab);
    let mut view = workspace(&keys);
    let mailbox = Mailbox::<ToView>::new().unwrap();
    let mut last_tick = view.frontend.now();
    let mut read = |view: &mut View| {
        view.read_key(mailbox.as_fd(), &mut last_tick).unwrap();
        view.model().screen
    };

    // Start-up has no boot environment to show the snapshots of yet.
    assert_eq!(read(&mut view), Screen::Startup);
    let model = view.model();
    assert_eq!(model.away, None);
    let later = Outcome::Warning(text!("snapshots-after-startup"));
    assert_eq!(model.notification, Some(later));
    view.draw().unwrap();
    let text = shown(&view);
    assert!(
        text.contains("snapshots show once start-up is over"),
        "{text}"
    );
    assert_eq!(read(&mut view), Screen::Pools);
    assert_eq!(view.model().notification, None);
    assert_eq!(read(&mut view), Screen::Startup);
    assert_eq!(read(&mut view), Screen::Log);
    assert_eq!(read(&mut view), Screen::Startup);
    assert_eq!(view.model().away, Some(Tab::Pools));
}

#[test]
fn a_tab_left_during_start_up_holds_the_countdown_and_stays_open() {
    let keys = Scripted::default();
    keys.typed("4");
    let mut view = workspace(&keys);
    let mailbox = Mailbox::<ToView>::new().unwrap();
    let mut last_tick = view.frontend.now();

    view.read_key(mailbox.as_fd(), &mut last_tick).unwrap();
    view.send(Event::StageStarted("Unlocking LUKS volumes".into()))
        .unwrap();
    // The Log tab shows what start-up said so far.
    let text = shown(&view);
    assert!(text.contains("START-UP SO FAR"), "{text}");
    assert!(text.contains("Unlocking LUKS volumes"), "{text}");
    assert!(!text.contains("Importing pools"), "{text}");
    assert!(text.contains("shows once start-up is over"), "{text}");
    stages_end(&mut view, StageState::Done);
    let model = menu(&mut view);

    // The tab stays open, with no countdown; the Boot tab's start-up view
    // became its main view, behind it.
    assert_eq!(
        (model.screen, model.countdown, model.away),
        (Screen::Log, None, None)
    );
    assert!(model.started);
    let model = press(model, Key::Char('1'));
    assert_eq!(model.screen, Screen::Main);
}

#[test]
fn back_and_the_boot_tab_return_to_start_up_while_it_runs() {
    let keys = Scripted::default();
    keys.typed("3");
    keys.press(Key::Escape);
    keys.typed("4");
    keys.press(Key::Tab);
    keys.typed("?");
    let mut view = workspace(&keys);
    let mailbox = Mailbox::<ToView>::new().unwrap();
    let mut last_tick = view.frontend.now();
    let mut read = |view: &mut View| {
        view.read_key(mailbox.as_fd(), &mut last_tick).unwrap();
        view.model().screen
    };

    assert_eq!(read(&mut view), Screen::Pools);
    assert_eq!(read(&mut view), Screen::Startup);
    assert_eq!(read(&mut view), Screen::Log);
    assert_eq!(read(&mut view), Screen::Startup);
    assert_eq!(read(&mut view), Screen::Help(Context::Startup));
    stages_end(&mut view, StageState::Done);
    let model = menu(&mut view);

    // Help opened from start-up becomes the main view's.
    assert_eq!(model.screen, Screen::Help(Context::Main));
    assert_eq!(model.countdown, None);
}

#[test]
fn a_failed_stage_needs_attention_on_the_boot_tab() {
    let keys = Scripted::default();
    let mut view = workspace(&keys);

    let failed = Event::StageFinished {
        stage: "Unlocking LUKS volumes".into(),
        state: StageState::Failed("no key".into()),
    };
    view.send(failed).unwrap();

    let text = shown(&view);
    assert!(text.contains("[1 Boot]"), "{text}");
    assert!(text.contains("! 1"), "{text}");
    assert!(text.contains("Unlocking LUKS volumes failed"), "{text}");
    assert!(text.contains("Unlocking LUKS volumes: no key"), "{text}");
}
