//! Start-up as the workspace's Boot tab, through the demo machine: the
//! tabs looked at while the stages run, what they ask of the machine, the
//! countdown once start-up is over, and the classic map's start-up, which
//! stays as it was.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::keymap::{Context, Map, Tab};
use crate::menu::model::{Dialog, Model, Screen};
use crate::menu::theme::Theme;
use crate::menu::world::Snapshot;

/// The demo at power-on, its first stage running.
fn powered_on(options: &DemoOptions) -> Session {
    let mut session = Session::new(options, Theme::new(Capabilities::FULL));
    session.tick(0);
    session
}

fn workspace() -> DemoOptions {
    DemoOptions {
        classic: false,
        ..DemoOptions::default()
    }
}

#[test]
fn tabs_left_during_start_up_ask_the_machine_nothing_and_hold_the_countdown() {
    let mut session = powered_on(&workspace());

    // On the Boot tab a digit is the first key of a passphrase: Tab and
    // Shift-Tab choose a tab there.
    session.key(Key::Char('2'));
    assert_eq!(session.model().screen, Screen::Startup);
    assert_eq!(session.model().away, None);
    session.key(Key::Tab);
    assert_eq!(session.model().screen, Screen::Snapshots);
    // Another tab still takes the digits.
    session.key(Key::Char('4'));
    assert_eq!(session.model().screen, Screen::Log);
    let model = session.model();
    // The tab left for first is kept: the Snapshots tab.
    assert_eq!((model.away, model.started), (Some(Tab::Snapshots), false));
    assert_eq!(
        (model.reading.as_ref(), model.reading_history),
        (None, false)
    );
    assert_eq!(model.notification, None);

    session.start();
    let model = session.model();
    assert!(model.started);
    assert_eq!(
        (model.screen, model.countdown, model.away),
        (Screen::Log, None, None)
    );
    // Nobody waits for a countdown: the first tick reads the machine.
    session.tick(200);
    assert!(session.model().scanned);
    assert!(session.model().history.is_some());
    session.key(Key::Char('1'));
    assert_eq!(session.model().screen, Screen::Main);
}

#[test]
fn help_and_moving_during_start_up_leave_the_countdown_armed() {
    let mut session = powered_on(&workspace());

    session.key(Key::Down);
    // `j` and `k` do not move there: they are the first key of a
    // passphrase.
    session.key(Key::Char('j'));
    session.key(Key::Char('k'));
    assert_eq!(session.model().screen, Screen::Startup);
    assert_eq!(session.model().away, None);
    session.key(Key::Char('?'));
    assert_eq!(session.model().screen, Screen::Help(Context::Startup));
    session.key(Key::Escape);
    assert_eq!(session.model().screen, Screen::Startup);
    // Keys of what start-up does not offer are not bound: none says so.
    session.key(Key::Char('x'));
    session.key(Key::Char('!'));
    assert_eq!(session.model().notification, None);
    assert_eq!(session.model().screen, Screen::Startup);

    session.start();
    assert_eq!(session.model().screen, Screen::Main);
    assert_eq!(session.model().countdown_seconds(), Some(10));
}

#[test]
fn a_secret_asked_while_another_tab_is_open_stays_hidden() {
    let mut session = powered_on(&DemoOptions {
        tpm_changed: true,
        ..workspace()
    });
    session.key(Key::BackTab);
    session.key(Key::BackTab);

    session.start();
    let reveal = |session: &mut Session| {
        session.press(KeyPress::new(Key::Char('r')).with_ctrl());
        match &session.model().dialog {
            Some(Dialog::Unlock(prompt)) => prompt.revealed,
            other => panic!("no unlock dialog: {other:?}"),
        }
    };
    assert_eq!(session.model().screen, Screen::Pools);
    assert!(!reveal(&mut session));

    session.type_text("demo");
    session.key(Key::Enter);
    session.start();
    assert_eq!(session.model().countdown, None);
    assert_eq!(session.model().screen, Screen::Pools);
}

#[test]
fn the_classic_map_s_start_up_has_no_tabs_and_ends_on_the_main_view() {
    let mut session = powered_on(&DemoOptions {
        classic: true,
        ..DemoOptions::default()
    });

    session.key(Key::Char('4'));
    assert_eq!(session.model().screen, Screen::Startup);
    session.key(Key::Char('l'));
    assert_eq!(session.model().screen, Screen::Log);
    assert_eq!(session.model().away, None);

    session.start();
    assert_eq!(session.model().screen, Screen::Main);
    assert_eq!(session.model().countdown_seconds(), Some(10));
}

#[test]
fn a_new_round_asks_for_the_history_again_and_keeps_the_tab_left_at_start_up() {
    let theme = Theme::new(Capabilities::FULL);
    let mut model = Model::new(Snapshot::default(), theme);
    model.map = Map::Workspace;
    model.started = true;
    model.reading_history = true;
    model.screen = Screen::Pools;
    model.away = Some(Tab::Pools);

    model.begin_round();

    assert_eq!(
        (model.screen, model.reading_history),
        (Screen::Pools, false)
    );
    model.away = None;
    model.started = true;
    model.begin_round();
    assert_eq!(model.screen, Screen::Main);
}
