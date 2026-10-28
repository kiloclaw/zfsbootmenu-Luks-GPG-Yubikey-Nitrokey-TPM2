//! The boot health and restore features of the prototype, through the
//! demo machine: checks that stop a boot, the fallback after failed boots,
//! the one-time boot, and restoring files from a snapshot.

use pretty_assertions::assert_eq;
use zbm_display::keys::Key;

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

fn selected(session: &Session) -> Option<String> {
    session
        .model()
        .selected_environment()
        .map(|e| e.name.to_string())
}

#[test]
fn a_failed_check_asks_before_booting() {
    let mut session = started();
    session.key(Key::Down);
    session.key(Key::Down);

    session.key(Key::Enter);

    assert_eq!(selected(&session).as_deref(), Some("tank/ROOT/debian"));
    assert_eq!(session.actions, []);
    let Some(Dialog::Choose { title, lines, .. }) = &session.model().dialog else {
        panic!("no sheet: {:?}", session.model().dialog);
    };
    assert_eq!(title, "tank/ROOT/debian May Not Boot");
    assert!(
        lines[0].starts_with("No zfs.ko for 6.1.0-26-amd64"),
        "{lines:?}"
    );

    session.key(Key::Enter);

    assert!(matches!(
        session.actions.as_slice(),
        [Action::Boot { environment, .. }] if environment.as_str() == "tank/ROOT/debian"
    ));
}

#[test]
fn a_boot_from_the_palette_asks_too() {
    let mut session = started();

    session.key(Key::Char(':'));
    session.type_text("Boot tank/ROOT/debian");
    session.key(Key::Enter);

    assert_eq!(session.actions, []);
    let Some(Dialog::Choose { title, .. }) = &session.model().dialog else {
        panic!("no sheet: {:?}", session.model().dialog);
    };
    assert_eq!(title, "tank/ROOT/debian May Not Boot");
    session.key(Key::Enter);
    assert!(matches!(
        session.actions.as_slice(),
        [Action::Boot { environment, .. }] if environment.as_str() == "tank/ROOT/debian"
    ));
}

#[test]
fn a_warning_does_not_stop_a_boot_and_cancel_closes_the_sheet() {
    let mut session = started();
    session.key(Key::Down);
    session.key(Key::Enter);
    assert!(matches!(session.actions.as_slice(), [Action::Boot { .. }]));

    let mut session = started();
    session.key(Key::Down);
    session.key(Key::Down);
    session.key(Key::Enter);
    session.key(Key::Down);
    session.key(Key::Enter);
    assert_eq!(session.actions, []);
    assert_eq!(session.model().dialog, None);
}

#[test]
fn after_failed_boots_the_countdown_boots_the_fallback() {
    let mut session = session(&DemoOptions {
        failing: true,
        ..DemoOptions::default()
    });

    for _ in 0..10 {
        session.tick(1000);
    }

    let snapshot = "zroot/ROOT/void@pre-upgrade".parse().unwrap();
    assert_eq!(session.actions, [Action::BootSnapshot(snapshot)]);
}

#[test]
fn the_palette_gives_the_boot_tries_back() {
    let mut session = session(&DemoOptions {
        failing: true,
        ..DemoOptions::default()
    });

    session.key(Key::Char(':'));
    session.type_text("tries back");
    session.key(Key::Enter);

    let name = "zroot/ROOT/void".parse().unwrap();
    assert_eq!(session.actions, [Action::ResetBootTries(name)]);
    let snapshot = &session.model().snapshot;
    assert_eq!(snapshot.fallback, None);
    let tries = snapshot.environments.iter().find_map(|e| e.tries);
    assert_eq!(tries.map(|t| t.left), Some(3));
}

#[test]
fn a_one_time_boot_is_selected_and_booted() {
    let mut session = session(&DemoOptions {
        next_boot: true,
        ..DemoOptions::default()
    });
    assert_eq!(selected(&session).as_deref(), Some("tank/ROOT/arch"));

    for _ in 0..10 {
        session.tick(1000);
    }

    assert!(matches!(
        session.actions.as_slice(),
        [Action::Boot { environment, .. }] if environment.as_str() == "tank/ROOT/arch"
    ));
}

/// The differences of the newest snapshot of the default environment.
fn diff() -> Session {
    let mut session = started();
    session.key(Key::Char('s'));
    session.key(Key::Char('o'));
    session.key(Key::Home);
    session.key(Key::Char('f'));
    session
}

#[test]
fn marked_files_are_restored_after_the_name_is_typed() {
    let mut session = diff();
    assert_eq!(session.model().screen, Screen::Diff);
    session.key(Key::Char(' '));
    for _ in 0..3 {
        session.key(Key::Down);
    }
    session.key(Key::Char(' '));

    session.key(Key::Enter);
    session.type_text("void");
    session.key(Key::Enter);

    let paths = vec!["/etc/fstab".to_owned(), "/usr/lib/libdrm.so.2".to_owned()];
    let snapshot = "zroot/ROOT/void@pre-upgrade".parse().unwrap();
    assert_eq!(
        session.actions,
        [
            Action::Diff(snapshot, None),
            Action::RestoreFiles {
                snapshot: "zroot/ROOT/void@pre-upgrade".parse().unwrap(),
                paths,
            }
        ]
    );
    let diff = session.model().diff.as_ref().unwrap();
    assert_eq!(diff.changes.len(), 4);
    assert!(diff.marked.is_empty());
    let done = matches!(
        &session.model().notification,
        Some(Outcome::Done(text)) if text.starts_with("2 files of zroot/ROOT/void restored")
    );
    assert!(done, "{:?}", session.model().notification);
}

#[test]
fn a_file_created_since_cannot_be_restored() {
    let mut session = diff();
    session.key(Key::Down);

    session.key(Key::Enter);

    assert_eq!(session.model().dialog, None);
    let warned = matches!(
        &session.model().notification,
        Some(Outcome::Warning(text)) if text.contains("nothing to restore")
    );
    assert!(warned);
}

#[test]
fn esc_returns_from_the_differences_to_the_snapshots() {
    let mut session = diff();

    session.key(Key::Escape);

    assert_eq!(session.model().screen, Screen::Snapshots);
    assert_eq!(session.model().diff, None);
}
