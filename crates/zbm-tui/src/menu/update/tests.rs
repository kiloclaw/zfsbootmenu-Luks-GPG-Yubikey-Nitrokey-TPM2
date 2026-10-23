use pretty_assertions::assert_eq;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::model::{Dialog, Screen};
use crate::menu::theme::Theme;
use crate::menu::world::{Action, Outcome};

fn session(options: &DemoOptions) -> Session {
    let mut session = Session::new(options, Theme::new(Capabilities::FULL));
    session.start();
    session
}

fn started() -> Session {
    session(&DemoOptions::default())
}

fn boot_action(environment: &str, kernel: Option<&str>, line: Option<&str>) -> Action {
    Action::Boot {
        environment: environment.parse().unwrap(),
        kernel: kernel.map(str::to_owned),
        command_line: line.map(str::to_owned),
    }
}

#[test]
fn start_up_ends_on_the_main_view_with_the_countdown() {
    let session = started();

    assert_eq!(session.model().screen, Screen::Main);
    assert_eq!(session.model().countdown_seconds(), Some(10));
    assert_eq!(
        session
            .model()
            .selected_environment()
            .map(|e| e.name.as_str()),
        Some("zroot/ROOT/void")
    );
}

#[test]
fn t1_the_countdown_boots_the_default_without_a_key() {
    let mut session = started();

    for _ in 0..10 {
        session.tick(1000);
    }

    assert_eq!(session.keys(), 0);
    assert_eq!(
        session.actions,
        [boot_action("zroot/ROOT/void", None, None)]
    );
    assert!(
        session
            .leaving()
            .is_some_and(|why| why.starts_with("Booting zroot/ROOT/void"))
    );
}

#[test]
fn any_key_stops_the_countdown_and_esc_does_nothing_else() {
    let mut session = started();

    session.key(KeyCode::Esc);
    session.tick(20_000);

    assert_eq!(session.model().countdown, None);
    assert_eq!(session.actions, []);
}

#[test]
fn t2_another_environment_boots_in_two_keys() {
    let mut session = started();

    session.key(KeyCode::Down);
    session.key(KeyCode::Enter);

    assert_eq!(session.keys(), 2);
    assert_eq!(session.actions, [boot_action("tank/ROOT/arch", None, None)]);
}

#[test]
fn t2_the_filter_finds_an_environment() {
    let mut session = started();

    session.key(KeyCode::Char('/'));
    session.type_text("ubu");
    session.key(KeyCode::Enter);

    assert_eq!(
        session.actions,
        [boot_action("zroot/ROOT/ubuntu", None, None)]
    );
}

#[test]
fn t3_another_kernel_boots_in_three_keys() {
    let mut session = started();

    session.key(KeyCode::Char('k'));
    session.key(KeyCode::Down);
    session.key(KeyCode::Enter);

    assert_eq!(session.keys(), 3);
    assert_eq!(
        session.actions,
        [boot_action("zroot/ROOT/void", Some("6.12.8_1"), None)]
    );
}

#[test]
fn t4_a_snapshot_boots_as_a_clone() {
    let mut session = started();

    session.key(KeyCode::Char('s'));
    session.key(KeyCode::Char('o'));
    session.key(KeyCode::Char('b'));

    assert_eq!(session.keys(), 3);
    let snapshot = "zroot/ROOT/void@pre-upgrade".parse().unwrap();
    assert_eq!(session.actions, [Action::BootSnapshot(snapshot)]);
}

#[test]
fn t4_a_rollback_needs_the_name_typed() {
    let mut session = started();
    session.key(KeyCode::Char('s'));
    session.key(KeyCode::Char('o'));

    session.key(KeyCode::Char('r'));
    session.type_text("voi");
    session.key(KeyCode::Enter);
    let refused = session.actions.clone();
    session.type_text("d");
    session.key(KeyCode::Enter);

    assert_eq!(refused, []);
    let snapshot = "zroot/ROOT/void@pre-upgrade".parse().unwrap();
    assert_eq!(
        session.actions,
        [Action::Rollback {
            snapshot,
            safety: true
        }]
    );
    let done = matches!(
        &session.model().notification,
        Some(Outcome::Done(text)) if text.contains("rolled back")
    );
    assert!(done);
}

#[test]
fn t5_volumes_unlock_with_the_secret_and_enter() {
    let mut session = session(&DemoOptions {
        locked: true,
        ..DemoOptions::default()
    });
    assert!(matches!(
        session.model().dialog,
        Some(Dialog::Unlock { .. })
    ));

    session.type_text("1234");
    session.key(KeyCode::Enter);
    session.start();
    session.key(KeyCode::Enter);
    session.start();
    session.key(KeyCode::Down);
    session.type_text("demo");
    session.key(KeyCode::Enter);
    session.start();
    session.type_text("demo");
    session.key(KeyCode::Enter);
    session.start();

    assert_eq!(session.model().screen, Screen::Main);
    let open = session
        .model()
        .snapshot
        .volumes
        .iter()
        .all(|v| v.state == crate::menu::world::VolumeState::Open);
    assert!(open);
}

#[test]
fn t6_the_command_line_is_edited_for_one_boot() {
    let mut session = started();

    session.key(KeyCode::Char('e'));
    session.type_text(" systemd.unit=rescue.target");
    session.key(KeyCode::Enter);

    let line = "quiet loglevel=4 systemd.unit=rescue.target";
    assert_eq!(
        session.actions,
        [boot_action("zroot/ROOT/void", None, Some(line))]
    );
}

#[test]
fn an_unclosed_quote_stops_the_boot_with_a_warning() {
    let mut session = started();

    session.key(KeyCode::Char('e'));
    session.type_text(" init=\"/bin/sh");
    session.key(KeyCode::Enter);

    assert_eq!(session.actions, []);
    assert!(matches!(
        session.model().notification,
        Some(Outcome::Warning(_))
    ));
}

#[test]
fn t7_a_foreign_pool_imports_with_its_host_id_in_two_keys() {
    let mut session = session(&DemoOptions {
        foreign: true,
        ..DemoOptions::default()
    });
    assert!(matches!(
        session.model().dialog,
        Some(Dialog::Choose { .. })
    ));

    session.key(KeyCode::Down);
    session.key(KeyCode::Enter);
    session.start();

    assert_eq!(session.model().screen, Screen::Main);
    assert!(
        session
            .model()
            .snapshot
            .pools
            .iter()
            .all(|pool| pool.imported)
    );
}

#[test]
fn a_failed_import_shows_what_to_do_next() {
    let mut session = session(&DemoOptions {
        foreign: true,
        ..DemoOptions::default()
    });

    session.key(KeyCode::Enter);

    let Some(Dialog::Error { next, .. }) = &session.model().dialog else {
        panic!("no error dialog: {:?}", session.model().dialog);
    };
    assert_eq!(next.len(), 3);
}

#[test]
fn the_palette_finds_a_snapshot_among_hundreds() {
    let mut session = started();

    session.key(KeyCode::Char(':'));
    session.type_text("2026-08-31");
    session.key(KeyCode::Enter);

    let snapshot = "zroot/ROOT/void@autosnap_2026-08-31_daily".parse().unwrap();
    assert_eq!(session.actions, [Action::BootSnapshot(snapshot)]);
}

#[test]
fn classic_keys_work_with_alt() {
    let mut session = started();

    session.press(KeyCode::Char('k'), KeyModifiers::ALT);

    assert_eq!(session.model().screen, Screen::Kernels);
}

#[test]
fn help_returns_to_the_view_it_was_opened_from() {
    let mut session = started();
    session.key(KeyCode::Char('s'));

    session.key(KeyCode::Char('?'));
    let help = session.model().screen;
    session.key(KeyCode::Esc);

    assert_eq!(help, Screen::Help(crate::menu::keymap::Context::Snapshots));
    assert_eq!(session.model().screen, Screen::Snapshots);
}

#[test]
fn power_offers_three_choices() {
    let mut session = started();

    session.key(KeyCode::Char('x'));
    session.key(KeyCode::Down);
    session.key(KeyCode::Enter);

    assert_eq!(session.actions, [Action::Reboot]);
    assert_eq!(session.leaving(), Some("Rebooting"));
}

#[test]
fn setting_the_default_moves_the_mark_and_reports_it() {
    let mut session = started();
    session.key(KeyCode::Down);

    session.key(KeyCode::Char('d'));

    let defaults: Vec<&str> = session
        .model()
        .snapshot
        .environments
        .iter()
        .filter(|e| e.is_default)
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(defaults, ["zroot/ROOT/void", "tank/ROOT/arch"]);
    let selected = session
        .model()
        .selected_environment()
        .map(|e| e.name.as_str());
    assert_eq!(selected, Some("tank/ROOT/arch"));
    assert!(
        matches!(&session.model().notification, Some(Outcome::Done(t)) if t.contains("read/write"))
    );
}

#[test]
fn help_scrolls_and_opens_at_the_top() {
    let mut session = started();
    session.key(KeyCode::Esc);
    session.key(KeyCode::Char('?'));

    session.key(KeyCode::Down);
    session.key(KeyCode::Down);
    assert_eq!(session.model().scroll, 2);
    session.key(KeyCode::End);
    session.key(KeyCode::Down);
    let end = session.model().scroll;
    session.key(KeyCode::Esc);
    session.key(KeyCode::Char('?'));

    assert!(end > 2);
    assert_eq!(session.model().scroll, 0);
}

#[test]
fn a_new_filter_selects_the_best_match() {
    let mut session = started();
    session.key(KeyCode::Down);
    session.key(KeyCode::Down);

    session.key(KeyCode::Char('/'));
    session.type_text("ubu");

    let selected = session
        .model()
        .selected_environment()
        .map(|e| e.name.as_str());
    assert_eq!(selected, Some("zroot/ROOT/ubuntu"));
}
