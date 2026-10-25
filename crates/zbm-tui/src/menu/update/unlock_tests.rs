//! Unlocking and the packages since a snapshot, through the demo machine:
//! the TPM that kept a key and sealing it again, typing with another
//! keyboard layout, and the packages segment of the changes.

use pretty_assertions::assert_eq;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Message, update};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session, data};
use crate::menu::keymap::Context;
use crate::menu::model::{Dialog, DiffView, Model, Prompt, Screen, Segment};
use crate::menu::theme::Theme;
use crate::menu::view::tests::screen;
use crate::menu::world::{Action, Method, Notice, Outcome};

fn session(options: &DemoOptions) -> Session {
    let mut session = Session::new(options, Theme::new(Capabilities::FULL));
    session.start();
    session
}

fn tpm_changed() -> Session {
    session(&DemoOptions {
        tpm_changed: true,
        ..DemoOptions::default()
    })
}

fn locked() -> Session {
    session(&DemoOptions {
        locked: true,
        ..DemoOptions::default()
    })
}

fn prompt(session: &Session) -> &Prompt {
    match &session.model().dialog {
        Some(Dialog::Unlock(prompt)) => prompt,
        other => panic!("no unlock dialog: {other:?}"),
    }
}

fn ctrl(session: &mut Session, c: char) {
    session.press(KeyCode::Char(c), KeyModifiers::CONTROL);
}

fn kept(session: &Session) -> bool {
    session.model().snapshot.volumes[0]
        .tpm2
        .as_ref()
        .is_some_and(|seal| !seal.is_current())
}

#[test]
fn a_changed_pcr_asks_for_the_passphrase_and_says_why() {
    let session = tpm_changed();

    let prompt = prompt(&session);
    assert_eq!(prompt.volume, "luks-zroot-a");
    assert_eq!(prompt.method, Method::Passphrase);
    let notice = prompt.notice.as_ref().map(Notice::text);
    assert_eq!(
        notice,
        Some("The TPM kept the key. PCR 0 (firmware code): the firmware was updated to 1.21.")
    );
}

#[test]
fn the_key_is_sealed_again_once_the_name_is_typed() {
    let mut session = tpm_changed();
    session.type_text("demo");
    session.key(KeyCode::Enter);

    let Some(Dialog::Confirm { title, name, .. }) = &session.model().dialog else {
        panic!("no confirmation: {:?}", session.model().dialog);
    };
    assert_eq!(title, "Re-seal the TPM2 Key of luks-zroot-a");
    assert_eq!(name, "luks-zroot-a");

    session.type_text("luks-zroot-a");
    session.key(KeyCode::Enter);

    assert_eq!(
        session.actions.last(),
        Some(&Action::Reseal("luks-zroot-a".into()))
    );
    assert!(!kept(&session));
    assert!(matches!(
        &session.model().notification,
        Some(Outcome::Done(text)) if text.contains("sealed to PCRs 0 and 7")
    ));
}

#[test]
fn not_now_lets_start_up_finish_and_the_palette_offers_it_later() {
    let mut session = tpm_changed();
    session.type_text("demo");
    session.key(KeyCode::Enter);
    session.key(KeyCode::Esc);
    session.start();

    assert!(session.model().started);
    assert!(kept(&session));
    session.key(KeyCode::Char(':'));
    session.type_text("re-seal");
    session.key(KeyCode::Enter);

    assert!(matches!(
        &session.model().dialog,
        Some(Dialog::Confirm { title, .. }) if title.starts_with("Re-seal")
    ));
}

#[test]
fn the_tpm_pin_is_not_asked_while_the_tpm_keeps_the_key() {
    let mut session = tpm_changed();
    session.key(KeyCode::Down);
    assert_eq!(prompt(&session).method, Method::Tpm2Pin);

    session.type_text("1234");
    session.key(KeyCode::Enter);

    let prompt = prompt(&session);
    assert_eq!(prompt.method, Method::Passphrase);
    assert!(matches!(prompt.notice, Some(Notice::Instead(_))));
}

#[test]
fn a_wrong_passphrase_suggests_the_keyboard_layout() {
    let mut session = locked();
    session.key(KeyCode::Down);
    session.type_text("dmeo");
    session.key(KeyCode::Enter);

    assert!(matches!(prompt(&session).notice, Some(Notice::Wrong(_))));
    let text = screen(session.model(), 80, 25);
    assert!(text.contains("Set with another layout?"), "{text}");
}

#[test]
fn another_layout_translates_keys_and_clears_what_was_typed() {
    let mut session = locked();
    session.key(KeyCode::Down);
    session.type_text("abc");

    ctrl(&mut session, 'k');

    assert_eq!(session.model().layout(), Some("de"));
    assert_eq!(prompt(&session).typed(), "");
    session.type_text("Y[rich");
    assert_eq!(prompt(&session).typed(), "Zürich");
    assert!(!screen(session.model(), 80, 25).contains("Zürich"));

    // Start-up never shows what is typed: it would leave copies of the
    // secret in memory nothing wipes.
    ctrl(&mut session, 'r');

    assert!(!prompt(&session).revealed);
    assert!(!screen(session.model(), 80, 25).contains("Zürich"));
    session.key(KeyCode::Backspace);
    assert_eq!(prompt(&session).typed(), "Züric");
    for _ in 0..3 {
        ctrl(&mut session, 'k');
    }
    assert_eq!(session.model().layout(), Some("us"));
}

/// Keys follow the layout everywhere, view keys too, so the key labelled
/// `/` on a German keyboard filters.
#[test]
fn the_layout_applies_to_every_text_field() {
    let mut session = locked();
    session.key(KeyCode::Down);
    ctrl(&mut session, 'k');
    session.type_text("demo");
    session.key(KeyCode::Enter);
    session.start();
    while session.model().dialog.is_some() {
        session.key(KeyCode::Esc);
        session.start();
    }
    // A German keyboard has / on Shift-7, which a US console sends as &.
    session.key(KeyCode::Char('&'));

    session.type_text("y");

    assert_eq!(session.model().environments.filter, "z");
}

/// A model on the demo machine whose keyboard is `layouts`, local or not,
/// with an unlock dialog open.
fn keyboard_model(layouts: &[&str], local: bool) -> Model {
    let mut snapshot = data::machine(&DemoOptions::default());
    snapshot.keyboard.layouts = layouts.iter().map(|l| (*l).to_owned()).collect();
    snapshot.keyboard.local = local;
    let mut model = Model::new(snapshot, Theme::new(Capabilities::FULL));
    let prompt = Prompt::new("luks-tank".into(), Method::Passphrase, None, None);
    model.dialog = Some(Dialog::Unlock(prompt));
    model
}

fn press(model: Model, code: KeyCode, modifiers: KeyModifiers) -> Model {
    update(model, Message::Key(KeyEvent::new(code, modifiers))).0
}

#[test]
fn a_serial_terminal_or_a_single_layout_is_not_translated() {
    for (layouts, local, says) in [
        (&["us", "de"][..], false, "serial terminal"),
        (&["us"][..], true, "one keyboard layout"),
    ] {
        let model = keyboard_model(layouts, local);
        let model = press(model, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert!(matches!(
            &model.notification,
            Some(Outcome::Warning(text)) if text.contains(says)
        ));
        let model = press(model, KeyCode::Char('y'), KeyModifiers::NONE);

        assert_eq!(model.layout, 0);
        let Some(Dialog::Unlock(prompt)) = &model.dialog else {
            panic!("the dialog closed");
        };
        assert_eq!(prompt.typed(), "y");
        let text = screen(&model, 80, 25);
        let line = if local {
            "Keyboard: us"
        } else {
            "the terminal's own"
        };
        assert!(text.contains(line), "{text}");
    }
}

fn packages() -> Session {
    let mut session = session(&DemoOptions::default());
    for c in ['s', 'o', 'f'] {
        session.key(KeyCode::Char(c));
    }
    session.key(KeyCode::Right);
    session
}

fn diff(session: &Session) -> &DiffView {
    session.model().diff.as_ref().expect("the changes are open")
}

#[test]
fn the_packages_segment_lists_what_the_upgrade_changed() {
    let mut session = packages();

    assert_eq!(session.model().context(), Context::Packages);
    let names: Vec<&str> = diff(&session)
        .package_changes()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["libdrm", "nvidia", "mesa", "zfs", "linux-firmware"]);

    session.key(KeyCode::Char(' '));
    assert!(diff(&session).marked.is_empty());
    session.key(KeyCode::Char('/'));
    session.type_text("zf");

    session.key(KeyCode::Enter);

    assert_eq!(diff(&session).segment, Segment::Files);
    assert_eq!(
        diff(&session).selected().map(|c| c.path.as_str()),
        Some("/boot/initramfs-6.12.9_1.img")
    );
    session.key(KeyCode::Left);
    assert_eq!(diff(&session).segment, Segment::Packages);
}

#[test]
fn a_restore_keeps_the_segment_and_no_database_says_so() {
    let mut session = packages();
    session.key(KeyCode::Left);
    session.key(KeyCode::Enter);
    session.type_text("void");
    session.key(KeyCode::Enter);
    assert_eq!(diff(&session).segment, Segment::Files);
    assert_eq!(session.model().screen, Screen::Diff);

    let mut model = session.model().clone();
    if let Some(diff) = &mut model.diff {
        diff.packages = None;
        diff.segment = Segment::Packages;
        diff.refilter();
    }
    let text = screen(&model, 80, 25);
    assert!(text.contains("No Package Database"), "{text}");
    assert!(text.contains("Packages none"), "{text}");
}
