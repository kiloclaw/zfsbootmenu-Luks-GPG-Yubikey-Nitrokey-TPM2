//! Unlocking and the packages since a snapshot, through the demo machine:
//! the TPM that kept a key and sealing it again, typing with another
//! keyboard layout (the one each key was typed on, `Message::TypedOn`, and
//! the one `Ctrl-K` chose: what is translated, from which layout, and
//! where), and the packages segment of the changes.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::{Message, update};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session, data};
use crate::menu::keymap::Context;
use crate::menu::model::{Dialog, DiffView, Make, Model, Naming, Prompt, Screen, Segment, TypedOn};
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
    session.press(KeyPress::new(Key::Char(c)).with_ctrl());
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
fn the_session_keeps_no_secret_typed() {
    let mut session = tpm_changed();
    session.type_text("demo");
    session.key(Key::Enter);

    let unlocks: Vec<&Action> = session
        .actions
        .iter()
        .filter(|action| matches!(action, Action::Unlock { .. }))
        .collect();
    assert_eq!(unlocks.len(), 1, "{:?}", session.actions);
    assert!(matches!(
        unlocks[0],
        Action::Unlock { volume, secret: None, .. } if volume == "luks-zroot-a"
    ));
}

#[test]
fn the_key_is_sealed_again_once_the_name_is_typed() {
    let mut session = tpm_changed();
    session.type_text("demo");
    session.key(Key::Enter);

    let Some(Dialog::Confirm { title, name, .. }) = &session.model().dialog else {
        panic!("no confirmation: {:?}", session.model().dialog);
    };
    assert_eq!(title, "Re-seal the TPM2 Key of luks-zroot-a");
    assert_eq!(name, "luks-zroot-a");

    session.type_text("luks-zroot-a");
    session.key(Key::Enter);

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
    session.key(Key::Enter);
    session.key(Key::Escape);
    session.start();

    assert!(session.model().started);
    assert!(kept(&session));
    session.key(Key::Char(':'));
    session.type_text("re-seal");
    session.key(Key::Enter);

    assert!(matches!(
        &session.model().dialog,
        Some(Dialog::Confirm { title, .. }) if title.starts_with("Re-seal")
    ));
}

#[test]
fn the_tpm_pin_is_not_asked_while_the_tpm_keeps_the_key() {
    let mut session = tpm_changed();
    session.key(Key::Down);
    assert_eq!(prompt(&session).method, Method::Tpm2Pin);

    session.type_text("1234");
    session.key(Key::Enter);

    let prompt = prompt(&session);
    assert_eq!(prompt.method, Method::Passphrase);
    assert!(matches!(prompt.notice, Some(Notice::Instead(_))));
}

#[test]
fn a_wrong_passphrase_suggests_the_keyboard_layout() {
    let mut session = locked();
    session.key(Key::Down);
    session.type_text("dmeo");
    session.key(Key::Enter);

    assert!(matches!(prompt(&session).notice, Some(Notice::Wrong(_))));
    let text = screen(session.model(), 80, 25);
    assert!(text.contains("Set with another layout?"), "{text}");
}

#[test]
fn another_layout_translates_keys_and_clears_what_was_typed() {
    let mut session = locked();
    session.key(Key::Down);
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
    session.key(Key::Backspace);
    assert_eq!(prompt(&session).typed(), "Züric");
    for _ in 0..3 {
        ctrl(&mut session, 'k');
    }
    assert_eq!(session.model().layout(), Some("us"));
}

/// The chosen layout translates action keys too, so the key labelled `/`
/// on a German keyboard filters; the filter takes characters as typed.
#[test]
fn the_chosen_layout_translates_action_keys_but_not_the_filter() {
    let mut session = locked();
    session.key(Key::Down);
    ctrl(&mut session, 'k');
    session.type_text("demo");
    session.key(Key::Enter);
    session.start();
    while session.model().dialog.is_some() {
        session.key(Key::Escape);
        session.start();
    }
    // A German keyboard has / on Shift-7, which a US console sends as &.
    session.key(Key::Char('&'));

    session.type_text("y");

    assert_eq!(session.model().environments.filter, "y");
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

fn press(model: Model, key: KeyPress) -> Model {
    update(model, Message::Key(key)).0
}

#[test]
fn a_serial_terminal_or_a_single_layout_is_not_translated() {
    for (layouts, local, says) in [
        (&["us", "de"][..], false, "from a terminal"),
        (&["us"][..], true, "one keyboard layout"),
    ] {
        let model = keyboard_model(layouts, local);
        let model = press(model, KeyPress::new(Key::Char('k')).with_ctrl());
        assert!(matches!(
            &model.notification,
            Some(Outcome::Warning(text)) if text.contains(says)
        ));
        let model = press(model, Key::Char('y').into());

        assert_eq!(model.layout, None);
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

/// [`keyboard_model`] with `us`, `de` and `fr` on the machine's own
/// keyboard, its keys typed on `typed_on`.
fn typed_on(typed_on: Option<TypedOn>) -> Model {
    let model = keyboard_model(&["us", "de", "fr"], true);
    update(model, Message::TypedOn(typed_on)).0
}

fn on(name: &str) -> TypedOn {
    TypedOn::Layout(name.to_owned())
}

fn ctrl_k(model: Model) -> Model {
    press(model, KeyPress::new(Key::Char('k')).with_ctrl())
}

fn typed(model: Model, text: &str) -> Model {
    text.chars()
        .fold(model, |model, c| press(model, Key::Char(c).into()))
}

/// What the unlock dialog of `model` has typed.
fn secret(model: &Model) -> String {
    match &model.dialog {
        Some(Dialog::Unlock(prompt)) => prompt.typed().to_owned(),
        other => panic!("no unlock dialog: {other:?}"),
    }
}

/// `model` on the Boot tab, its dialog, filter and palette closed.
fn closed(mut model: Model) -> Model {
    model.dialog = None;
    model.palette = None;
    model.environments.filtering = false;
    model.screen = Screen::Main;
    model
}

#[test]
fn super_space_alone_translates_nothing() {
    // The console switched to German: the keys say so, and nothing chose
    // a layout to translate them into.
    let model = typed(typed_on(Some(on("de"))), "zy");

    assert_eq!(model.layout, None);
    assert_eq!(model.layout(), Some("de"));
    assert_eq!(secret(&model), "zy");
    assert!(screen(&model, 80, 25).contains("Keyboard: de"));
    let model = typed(press(closed(model), Key::Char('/').into()), "zy");
    assert_eq!(model.environments.filter, "zy");
    let model = typed(press(closed(model), Key::Char(':').into()), "zy");
    assert_eq!(model.palette.as_ref().map(|p| p.query.as_str()), Some("zy"));
}

#[test]
fn the_chosen_layout_translates_from_the_layout_a_key_was_typed_on() {
    let model = ctrl_k(typed_on(Some(on("de"))));
    assert_eq!(model.layout(), Some("fr"));

    // Z on a German keyboard is the key of the French Y, and Y that of W.
    let model = typed(model, "zy");
    assert_eq!(secret(&model), "yw");

    // Keys of a keyboard that types on French already are as typed.
    let model = typed(update(model, Message::TypedOn(Some(on("fr")))).0, "zy");
    assert_eq!(secret(&model), "ywzy");
}

#[test]
fn ctrl_k_starts_after_the_layout_keys_are_typed_on() {
    let chosen = |model: &Model| model.layout().map(str::to_owned);

    let model = ctrl_k(typed_on(Some(on("de"))));
    assert_eq!((model.layout, chosen(&model)), (Some(2), Some("fr".into())));
    let model = ctrl_k(model);
    assert_eq!((model.layout, chosen(&model)), (Some(0), Some("us".into())));
    // A layout the image does not list comes before its first.
    let model = ctrl_k(typed_on(Some(on("ru"))));
    assert_eq!(chosen(&model), Some("us".into()));
    // An input that does not say types on the console's own, the first.
    let model = ctrl_k(typed_on(None));
    assert_eq!(chosen(&model), Some("de".into()));
    assert!(matches!(
        &model.notification,
        Some(Outcome::Done(text)) if text.contains("de")
    ));
}

#[test]
fn a_terminal_s_characters_are_not_translated() {
    let model = ctrl_k(typed_on(Some(TypedOn::Characters)));

    assert_eq!(model.layout, None);
    assert!(matches!(
        &model.notification,
        Some(Outcome::Warning(text)) if text.contains("from a terminal")
    ));
    assert!(screen(&model, 80, 25).contains("the terminal's own"));
    // Even with a layout chosen on the display's keyboard before.
    let mut model = model;
    model.layout = Some(1);
    assert_eq!(secret(&typed(model, "zy")), "zy");
}

#[test]
fn the_kernel_console_keeps_ui1_s_rule() {
    // No input says where keys are typed: they are the console's own
    // layout's, the first.
    let model = typed(ctrl_k(typed_on(None)), "zy");

    assert_eq!(secret(&model), "yz");
}

#[test]
fn the_menu_over_ssh_has_no_layout_to_choose() {
    let model = typed(
        ctrl_k(update(keyboard_model(&["us", "de"], false), Message::TypedOn(None)).0),
        "zy",
    );

    assert_eq!((model.layout, secret(&model)), (None, "zy".to_owned()));
    assert!(screen(&model, 80, 25).contains("the terminal's own"));
}

/// The fields that take text, which keep the characters as typed once
/// `Ctrl-K` chose a layout: the filter, the palette, a name and a typed
/// confirmation; an action key is translated still.
#[test]
fn after_ctrl_k_the_filter_the_palette_and_typed_names_take_characters_as_typed() {
    let chosen = closed(ctrl_k(typed_on(None)));
    assert_eq!(chosen.layout(), Some("de"));

    // A German keyboard has / on Shift-7, which a US console sends as &.
    let model = typed(press(chosen, Key::Char('&').into()), "zy");
    assert!(model.environments.filtering);
    assert_eq!(model.environments.filter, "zy");
    // And : on Shift-., which a US console sends as >.
    let model = typed(press(closed(model), Key::Char('>').into()), "zy");
    assert_eq!(model.palette.as_ref().map(|p| p.query.as_str()), Some("zy"));

    let mut model = closed(model);
    model.dialog = Some(Dialog::Confirm {
        title: String::new(),
        lines: Vec::new(),
        name: "zy".to_owned(),
        typed: String::new(),
        action: Action::Reboot,
    });
    let model = typed(model, "zy");
    assert!(matches!(&model.dialog, Some(Dialog::Confirm { typed, .. }) if typed == "zy"));

    let mut model = closed(model);
    let make = Make::Snapshot("zroot/ROOT/void".parse().unwrap());
    model.dialog = Some(Dialog::Name(Naming::new(
        make,
        0,
        String::new(),
        Vec::new(),
    )));
    let model = typed(model, "zy");
    let Some(Dialog::Name(naming)) = &model.dialog else {
        panic!("the name dialog closed");
    };
    assert!(naming.typed.ends_with("zy"), "{}", naming.typed);
}

/// The console on the display serves a serial terminal too, and the menu's
/// own console is that terminal: the keys of the display's keyboard say
/// their layout all the same, and are translated.
#[test]
fn a_keyboard_of_the_display_is_translated_when_the_console_is_a_serial_one() {
    let model = keyboard_model(&["us", "de", "fr"], false);
    let model = ctrl_k(update(model, Message::TypedOn(Some(on("de")))).0);

    assert_eq!(model.layout(), Some("fr"));
    assert!(screen(&model, 80, 25).contains("Keyboard: fr"));
    let model = typed(model, "zy");
    assert_eq!(secret(&model), "yw");
    // The serial terminal's keys are not.
    let model = update(model, Message::TypedOn(Some(TypedOn::Characters))).0;
    assert_eq!(secret(&typed(model, "zy")), "ywzy");
}

fn packages() -> Session {
    let mut session = session(&DemoOptions::default());
    for c in ['s', 'o', 'f'] {
        session.key(Key::Char(c));
    }
    session.key(Key::Right);
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

    session.key(Key::Char(' '));
    assert!(diff(&session).marked.is_empty());
    session.key(Key::Char('/'));
    session.type_text("zf");

    session.key(Key::Enter);

    assert_eq!(diff(&session).segment, Segment::Files);
    assert_eq!(
        diff(&session).selected().map(|c| c.path.get().as_str()),
        Some("/boot/initramfs-6.12.9_1.img")
    );
    session.key(Key::Left);
    assert_eq!(diff(&session).segment, Segment::Packages);
}

#[test]
fn a_restore_keeps_the_segment_and_no_database_says_so() {
    let mut session = packages();
    session.key(Key::Left);
    session.key(Key::Enter);
    session.type_text("void");
    session.key(Key::Enter);
    assert_eq!(diff(&session).segment, Segment::Files);
    assert_eq!(session.model().screen, Screen::Diff);

    let mut model = session.into_model();
    if let Some(diff) = &mut model.diff {
        diff.packages = None;
        diff.segment = Segment::Packages;
        diff.refilter();
    }
    let text = screen(&model, 80, 25);
    assert!(text.contains("No Package Database"), "{text}");
    assert!(text.contains("Packages none"), "{text}");
}
