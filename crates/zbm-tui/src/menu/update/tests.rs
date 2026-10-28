use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};
use zbm_types::Untrusted;

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
        kernel: kernel.map(|kernel| Untrusted::new(kernel.to_owned())),
        command_line: line.map(|line| Untrusted::new(line.to_owned())),
        countdown: false,
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
    // The countdown's boot, which tries no key source.
    let expected = Action::Boot {
        environment: "zroot/ROOT/void".parse().unwrap(),
        kernel: None,
        command_line: None,
        countdown: true,
    };
    assert_eq!(session.actions, [expected]);
    assert!(
        session
            .leaving()
            .is_some_and(|why| why.starts_with("Booting zroot/ROOT/void"))
    );
}

#[test]
fn any_key_stops_the_countdown_and_esc_does_nothing_else() {
    let mut session = started();

    session.key(Key::Escape);
    session.tick(20_000);

    assert_eq!(session.model().countdown, None);
    assert_eq!(session.actions, []);
}

#[test]
fn t2_another_environment_boots_in_two_keys() {
    let mut session = started();

    session.key(Key::Down);
    session.key(Key::Enter);

    assert_eq!(session.keys(), 2);
    assert_eq!(session.actions, [boot_action("tank/ROOT/arch", None, None)]);
}

#[test]
fn t2_the_filter_finds_an_environment() {
    let mut session = started();

    session.key(Key::Char('/'));
    session.type_text("ubu");
    session.key(Key::Enter);

    assert_eq!(
        session.actions,
        [boot_action("zroot/ROOT/ubuntu", None, None)]
    );
}

#[test]
fn t3_another_kernel_boots_in_three_keys() {
    let mut session = started();

    session.key(Key::Char('k'));
    session.key(Key::Down);
    session.key(Key::Enter);

    assert_eq!(session.keys(), 3);
    assert_eq!(
        session.actions,
        [boot_action("zroot/ROOT/void", Some("6.12.8_1"), None)]
    );
}

#[test]
fn t4_a_snapshot_boots_as_a_clone() {
    let mut session = started();

    session.key(Key::Char('s'));
    session.key(Key::Char('o'));
    session.key(Key::Home);
    session.key(Key::Char('b'));

    assert_eq!(session.keys(), 4);
    let snapshot = "zroot/ROOT/void@pre-upgrade".parse().unwrap();
    assert_eq!(session.actions, [Action::BootSnapshot(snapshot)]);
}

#[test]
fn t4_a_rollback_needs_the_name_typed() {
    let mut session = started();
    session.key(Key::Char('s'));
    session.key(Key::Char('o'));
    session.key(Key::Home);

    session.key(Key::Char('r'));
    session.type_text("voi");
    session.key(Key::Enter);
    let refused = session.actions.clone();
    session.type_text("d");
    session.key(Key::Enter);

    assert_eq!(refused, []);
    let snapshot = "zroot/ROOT/void@pre-upgrade".parse().unwrap();
    assert_eq!(session.actions, [Action::Rollback(snapshot)]);
    let done = matches!(
        &session.model().notification,
        Some(Outcome::Done(text)) if text.contains("rolled back")
    );
    assert!(done);
}

#[test]
fn a_promotion_needs_the_environment_s_name_typed_after_its_own() {
    let mut session = started();
    session.key(Key::Char('s'));
    session.key(Key::Char('o'));
    session.key(Key::Home);

    session.key(Key::Char('m'));
    session.key(Key::Enter);
    let named = session.actions.clone();
    let asks =
        matches!(&session.model().dialog, Some(Dialog::Confirm { name, .. }) if name == "void");
    session.type_text("void");
    session.key(Key::Enter);

    assert_eq!(named, []);
    assert!(asks, "{:?}", session.model().dialog);
    let snapshot = "zroot/ROOT/void@pre-upgrade".parse().unwrap();
    assert_eq!(
        session.actions,
        [Action::CloneAndPromote(
            snapshot,
            "zroot/ROOT/void_NEW".parse().unwrap()
        )]
    );
}

#[test]
fn new_environments_and_snapshots_are_named_first() {
    let mut session = started();
    session.key(Key::Char('s'));
    session.key(Key::Char('o'));
    session.key(Key::Home);
    session.key(Key::Char('c'));
    session.key(Key::Backspace);
    session.key(Key::Backspace);
    session.type_text("/X @");
    session.key(Key::Enter);

    let snapshot: zbm_types::SnapshotName = "zroot/ROOT/void@pre-upgrade".parse().unwrap();
    assert_eq!(
        session.actions,
        [Action::Clone(
            snapshot,
            "zroot/ROOT/void_NX".parse().unwrap()
        )]
    );

    // A name the menu knows is refused; an empty one too.
    session.key(Key::Char('c'));
    session.key(Key::Backspace);
    session.key(Key::Backspace);
    session.type_text("X");
    session.key(Key::Enter);
    assert_eq!(session.actions.len(), 1);
    let refused = matches!(
        &session.model().notification,
        Some(Outcome::Warning(text)) if text.contains("exists already")
    );
    assert!(refused);
    session.key(Key::Escape);
    session.key(Key::Char('n'));
    let Some(Dialog::Name(naming)) = &session.model().dialog else {
        panic!("no name dialog for a snapshot");
    };
    assert!(naming.full_name().starts_with("zroot/ROOT/void@20"));
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
    session.key(Key::Enter);
    session.start();
    session.key(Key::Enter);
    session.start();
    session.key(Key::Down);
    session.type_text("demo");
    session.key(Key::Enter);
    session.start();
    session.type_text("demo");
    session.key(Key::Enter);
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

    session.key(Key::Char('e'));
    session.type_text(" systemd.unit=rescue.target");
    session.key(Key::Enter);

    let line = "quiet loglevel=4 systemd.unit=rescue.target";
    assert_eq!(
        session.actions,
        [boot_action("zroot/ROOT/void", None, Some(line))]
    );
    // If that boot fails, the menu is there to say so: the editor gave its
    // line to the boot and is gone, so the main view is shown, not an
    // empty editor.
    assert_eq!(session.model().screen, Screen::Main);
}

#[test]
fn an_unclosed_quote_stops_the_boot_with_a_warning() {
    let mut session = started();

    session.key(Key::Char('e'));
    session.type_text(" init=\"/bin/sh");
    session.key(Key::Enter);

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

    session.key(Key::Down);
    session.key(Key::Enter);
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

    session.key(Key::Enter);

    let Some(Dialog::Error { next, .. }) = &session.model().dialog else {
        panic!("no error dialog: {:?}", session.model().dialog);
    };
    assert_eq!(next.len(), 3);
}

#[test]
fn the_palette_finds_a_snapshot_among_hundreds() {
    let mut session = started();

    session.key(Key::Char(':'));
    session.type_text("2026-08-31");
    session.key(Key::Enter);

    let snapshot = "zroot/ROOT/void@autosnap_2026-08-31_daily".parse().unwrap();
    assert_eq!(session.actions, [Action::BootSnapshot(snapshot)]);
}

#[test]
fn classic_keys_work_with_alt() {
    let mut session = started();

    session.press(KeyPress::new(Key::Char('k')).with_alt());

    assert_eq!(session.model().screen, Screen::Kernels);
}

#[test]
fn help_returns_to_the_view_it_was_opened_from() {
    let mut session = started();
    session.key(Key::Char('s'));

    session.key(Key::Char('?'));
    let help = session.model().screen;
    session.key(Key::Escape);

    assert_eq!(help, Screen::Help(crate::menu::keymap::Context::Snapshots));
    assert_eq!(session.model().screen, Screen::Snapshots);
}

#[test]
fn power_offers_three_choices() {
    let mut session = started();

    session.key(Key::Char('x'));
    session.key(Key::Down);
    session.key(Key::Enter);

    assert_eq!(session.actions, [Action::Reboot]);
    assert_eq!(session.leaving(), Some("Rebooting"));
}

#[test]
fn setting_the_default_moves_the_mark_and_reports_it() {
    let mut session = started();
    session.key(Key::Down);

    session.key(Key::Char('d'));

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
    session.key(Key::Escape);
    session.key(Key::Char('?'));

    session.key(Key::Down);
    session.key(Key::Down);
    assert_eq!(session.model().scroll, 2);
    session.key(Key::End);
    session.key(Key::Down);
    let end = session.model().scroll;
    session.key(Key::Escape);
    session.key(Key::Char('?'));

    assert!(end > 2);
    assert_eq!(session.model().scroll, 0);
}

#[test]
fn a_new_filter_selects_the_best_match() {
    let mut session = started();
    session.key(Key::Down);
    session.key(Key::Down);

    session.key(Key::Char('/'));
    session.type_text("ubu");

    let selected = session
        .model()
        .selected_environment()
        .map(|e| e.name.as_str());
    assert_eq!(selected, Some("zroot/ROOT/ubuntu"));
}

/// A new snapshot from the machine keeps the environment selected, even
/// when another one now sorts into its place: Enter must not boot that one.
#[test]
fn a_new_snapshot_keeps_the_selection_by_name() {
    let mut session = started();
    session.key(Key::Down);
    let model = session.into_model();
    let chosen = model.selected_environment().map(|e| e.name.clone());
    let mut snapshot = model.snapshot.clone();
    let mut newcomer = snapshot.environments[0].clone();
    newcomer.name = "zroot/ROOT/aaa".parse().unwrap();
    newcomer.is_default = false;
    snapshot.environments.insert(0, newcomer);

    let event = crate::menu::world::Event::Changed(snapshot);
    let (model, _) = super::update(model, super::Message::Event(Box::new(event)));

    assert!(chosen.is_some());
    assert_eq!(model.selected_environment().map(|e| e.name.clone()), chosen);
}

#[test]
fn a_screen_opened_from_the_palette_starts_at_its_top() {
    let mut model = started().into_model();
    model.scroll = 7;

    let _ = super::dialogs::follow(&mut model, crate::menu::model::Then::Open(Screen::Log));

    assert_eq!((model.screen, model.scroll), (Screen::Log, 0));
}
