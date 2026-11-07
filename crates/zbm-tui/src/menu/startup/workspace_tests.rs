//! Start-up as the workspace's Boot tab: the keys read between questions,
//! the tabs looked at meanwhile, and the countdown the menu then arms or
//! not; and the classic map's view, which reads no key between questions.

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
    keys.press(Key::BackTab);
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

/// The secret of the unlock dialog `view` asks with the keys typed ahead.
fn unlocked_with(view: &mut View) -> Vec<u8> {
    let answer = view.ask(&unlock_requested(None), &mut || None).unwrap();
    let Action::Unlock { secret, .. } = answer else {
        panic!("no secret: {answer:?}");
    };
    secret.unwrap().expose().to_vec()
}

#[test]
fn a_passphrase_whose_first_key_is_a_digit_waits_for_its_dialog() {
    let keys = Scripted::default();
    keys.typed("4hunter");
    keys.press(Key::Enter);
    let mut view = workspace(&keys);

    read_keys(&mut view, 8);
    // No tab was chosen: nobody is marked away.
    assert_eq!(
        (view.model().screen, view.model().away, view.model().keys),
        (Screen::Startup, None, 0)
    );

    assert_eq!(unlocked_with(&mut view), b"4hunter");
    assert!(view.typed_ahead.is_empty());
    stages_end(&mut view, StageState::Done);
    let model = menu(&mut view);
    assert_eq!(model.screen, Screen::Main);
    assert_eq!(model.countdown_seconds(), Some(10));
}

#[test]
fn a_passphrase_whose_first_key_is_a_question_mark_opens_help_and_is_lost() {
    let keys = Scripted::default();
    keys.typed("?4ab");
    let mut view = workspace(&keys);

    read_keys(&mut view, 1);
    assert_eq!(view.model().screen, Screen::Help(Context::Startup));
    // Help reads the next keys: its digit chooses a tab, which holds no
    // keys typed ahead either.
    read_keys(&mut view, 3);
    let model = view.model();
    assert_eq!((model.screen, model.away), (Screen::Log, Some(Tab::Log)));
    assert!(view.typed_ahead.is_empty());
}

#[test]
fn j_and_k_typed_before_a_prompt_wait_for_it() {
    for first in ["jade", "kiwi"] {
        let keys = Scripted::default();
        keys.typed(first);
        keys.press(Key::Enter);
        let mut view = workspace(&keys);

        read_keys(&mut view, 5);
        assert_eq!(
            (view.model().screen, view.model().away, view.model().keys),
            (Screen::Startup, None, 0)
        );

        assert_eq!(unlocked_with(&mut view), first.as_bytes());
        assert!(view.typed_ahead.is_empty());
    }
}

#[test]
fn only_tab_and_shift_tab_change_tabs_during_start_up() {
    // Tab passes over the Snapshots tab, shut until start-up is over.
    let chosen = [(Key::Tab, Screen::Pools), (Key::BackTab, Screen::Log)];
    for (code, screen) in chosen {
        let keys = Scripted::default();
        keys.press(code);
        let mut view = workspace(&keys);

        read_keys(&mut view, 1);
        assert_eq!(view.model().screen, screen, "{code:?}");
        assert!(view.typed_ahead.is_empty(), "{code:?}");
    }
    for digit in ['1', '2', '3', '4'] {
        let keys = Scripted::default();
        keys.typed(&digit.to_string());
        let mut view = workspace(&keys);

        read_keys(&mut view, 1);
        let model = view.model();
        assert_eq!(
            (model.screen, model.away, model.notification.as_ref()),
            (Screen::Startup, None, None),
            "{digit}"
        );
        assert_eq!(view.typed_ahead.len(), 1, "{digit}");
    }
}

#[test]
fn a_digit_on_another_tab_during_start_up_still_chooses_a_tab() {
    let keys = Scripted::default();
    keys.press(Key::BackTab);
    keys.typed("31");
    let mut view = workspace(&keys);
    let mailbox = Mailbox::<ToView>::new().unwrap();
    let mut last_tick = view.frontend.now();
    let mut read = |view: &mut View| {
        view.read_key(mailbox.as_fd(), &mut last_tick).unwrap();
        view.model().screen
    };

    assert_eq!(read(&mut view), Screen::Log);
    assert_eq!(read(&mut view), Screen::Pools);
    assert_eq!(read(&mut view), Screen::Startup);
    assert!(view.typed_ahead.is_empty());
    // The tab left for first is kept.
    assert_eq!(view.model().away, Some(Tab::Log));
}

#[test]
fn start_ups_view_shows_no_digit_and_the_other_tabs_show_theirs() {
    let keys = Scripted::default();
    keys.press(Key::BackTab);
    let mut view = workspace(&keys);

    // The tab bar names no key on start-up's view, where a digit is the
    // first key of a passphrase.
    let text = shown(&view);
    let bar = text.lines().find(|line| line.contains("[Boot]")).unwrap();
    assert!(bar.contains("[Boot] Snapshots  Pools  Log"), "{text}");
    for digit in ['1', '2', '3', '4'] {
        assert!(!bar.contains(digit), "{text}");
    }
    // On another tab during start-up the digits choose a tab, and show.
    read_keys(&mut view, 1);
    view.draw().unwrap();
    let text = shown(&view);
    assert!(
        text.contains("1 Boot  2 Snapshots  3 Pools [4 Log]"),
        "{text}"
    );
}

#[test]
fn start_up_help_lists_no_digits() {
    let said = |intent| crate::menu::keymap::describe(Context::Startup, intent);
    let help = crate::menu::keymap::help(Map::Workspace, Context::Startup, &|_| true, &said);
    let keys: Vec<&str> = help.iter().flat_map(|(keys, _)| keys.split(", ")).collect();

    for typed in ["1", "2", "3", "4", "j", "k"] {
        assert!(!keys.contains(&typed), "{typed}: {keys:?}");
    }
    for key in [text!("key-tab"), text!("key-shift-tab"), "?".to_owned()] {
        assert!(keys.contains(&key.as_str()), "{key}: {keys:?}");
    }
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
    keys.press(Key::BackTab);
    keys.typed("q1");
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
    keys.press(Key::Tab);
    keys.typed("2");
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

    // Tab passes over the Snapshots tab, and its digit, pressed on another
    // tab, says why: start-up has no boot environment to show the
    // snapshots of yet.
    assert_eq!(read(&mut view), Screen::Pools);
    assert_eq!(view.model().notification, None);
    assert_eq!(read(&mut view), Screen::Pools);
    let later = Outcome::Warning(text!("snapshots-after-startup"));
    assert_eq!(view.model().notification, Some(later));
    view.draw().unwrap();
    let text = shown(&view);
    assert!(
        text.contains("snapshots show once start-up is over"),
        "{text}"
    );
    assert_eq!(read(&mut view), Screen::Startup);
    assert_eq!(view.model().notification, None);
    assert_eq!(read(&mut view), Screen::Log);
    assert_eq!(read(&mut view), Screen::Startup);
    assert_eq!(view.model().away, Some(Tab::Pools));
}

#[test]
fn a_tab_left_during_start_up_holds_the_countdown_and_stays_open() {
    let keys = Scripted::default();
    keys.press(Key::BackTab);
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
    keys.press(Key::Tab);
    keys.press(Key::Escape);
    keys.press(Key::BackTab);
    keys.typed("1?");
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
    assert!(text.contains("[Boot]"), "{text}");
    assert!(text.contains("! 1"), "{text}");
    assert!(text.contains("Unlocking LUKS volumes failed"), "{text}");
    assert!(text.contains("Unlocking LUKS volumes: no key"), "{text}");
}
