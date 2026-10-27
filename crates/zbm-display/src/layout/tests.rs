//! The tests compile layouts from the XKB data and compose rules of the
//! machine that runs them (`xkb-data` and `libx11-data` on Debian and
//! Ubuntu), as the image build does.

#![allow(
    clippy::disallowed_methods,
    reason = "the compose rules are the machine's own file, as the image build reads it"
)]

use std::path::Path;
use std::sync::OnceLock;

use pretty_assertions::assert_eq;

use super::compile::{self, COMPOSE, XKB};
use super::*;
use crate::keys::{Decoder, Key};

/// The rules of dead keys, compiled once.
fn dead_keys() -> &'static str {
    static RULES: OnceLock<String> = OnceLock::new();
    RULES.get_or_init(|| {
        let compose =
            std::fs::read(COMPOSE).expect("the compose rules of en_US.UTF-8 (libx11-data)");
        compile::dead_keys(&compose).unwrap()
    })
}

/// The keymap of `name`, compiled from the machine's XKB data.
fn keymap(name: &str) -> String {
    compile::keymap(Path::new(XKB), name).expect("the XKB data (xkb-data)")
}

/// The German layout, with its dead keys.
pub(crate) fn german() -> Layout {
    Layout::from_keymap("de", keymap("de").as_bytes())
        .unwrap()
        .with_dead_keys(dead_keys().as_bytes())
        .unwrap()
}

/// What pressing and releasing each key of `codes` types; key code 0 and
/// the next code hold that key while the one after is pressed.
fn typed(layout: &mut Layout, codes: &[u16]) -> String {
    let mut text = String::new();
    let mut codes = codes.iter();
    while let Some(&code) = codes.next() {
        let held = (code == 0).then(|| *codes.next().unwrap());
        let code = if let Some(held) = held {
            layout.key(held, 1);
            *codes.next().unwrap()
        } else {
            code
        };
        if let Some(typed) = layout.key(code, 1) {
            text.extend(typed.before);
            text.extend(typed.char);
        }
        assert_eq!(layout.key(code, 0), None);
        if let Some(held) = held {
            layout.key(held, 0);
        }
    }
    text
}

#[test]
fn names_are_layouts_with_an_optional_variant() {
    assert_eq!(compile::split("de"), Ok(("de", "")));
    assert_eq!(compile::split("de(nodeadkeys)"), Ok(("de", "nodeadkeys")));
    assert_eq!(compile::split("us(dvorak-intl)"), Ok(("us", "dvorak-intl")));
    for wrong in ["", "de(", "de()", "../de", "de(x/y)", "de nodeadkeys"] {
        let error = compile::split(wrong).unwrap_err();
        assert!(error.to_string().contains("not a layout name"), "{wrong}");
    }
}

#[test]
fn unknown_layouts_and_broken_keymaps_are_errors() {
    let unknown = compile::keymap(Path::new(XKB), "zz").unwrap_err();
    assert_eq!(unknown, LayoutError::Unknown("zz".to_owned()));
    assert_eq!(unknown.to_string(), "zz: no such keyboard layout");
    assert!(compile::keymap(Path::new(XKB), "de(").is_err());
    let broken = Layout::from_keymap("de", b"xkb_keymap {").unwrap_err();
    assert!(broken.to_string().starts_with("de: "), "{broken}");
}

#[test]
fn a_layout_types_its_characters() {
    let mut layout = german();

    assert_eq!(layout.name(), "de");
    assert_eq!(format!("{layout:?}"), "Layout { name: \"de\", .. }");
    // y z, Shift-2, AltGr-q, Caps Lock then a, Caps Lock again then a
    let text = typed(&mut layout, &[21, 44, 0, 42, 3, 0, 100, 16, 58, 30, 58, 30]);
    assert_eq!(text, "zy\"@Aa");
    layout.key(29, 1);
    let control = layout.key(37, 1).unwrap();
    assert_eq!(
        (control.char, control.ctrl, control.shift),
        (Some('k'), true, false)
    );
    assert_eq!(layout.key(37, 2).unwrap().char, Some('k'));
    layout.key(37, 0);
    layout.key(29, 0);
    let modifier = layout.key(42, 1).unwrap();
    assert_eq!((modifier.char, modifier.shift), (None, true));
}

#[test]
fn dead_keys_compose_with_the_next_key() {
    let mut layout = german();

    // ^ then e, ^ then space, ´ (Shift ´ is `) then a, ^ then x, which
    // types both, as the kernel's keymap does
    let text = typed(
        &mut layout,
        &[41, 18, 41, 57, 13, 30, 0, 42, 13, 18, 41, 45],
    );
    assert_eq!(text, "ê^áè^x");
    let mut plain = Layout::from_keymap("de", keymap("de").as_bytes()).unwrap();
    assert_eq!(typed(&mut plain, &[41, 18]), "e");
    let rules = dead_keys();
    assert!(
        rules.contains("<dead_circumflex> <e> : \"ê\"\n"),
        "{}",
        &rules[..200]
    );
    assert!(
        rules
            .lines()
            .all(|rule| rule.starts_with("<dead_") || rule.starts_with("<Multi_key>"))
    );
}

#[test]
fn the_compose_key_starts_sequences_and_types_nothing_itself() {
    let mut layout = german();
    // Right Alt is the compose key, not AltGr: a then e is æ, and q stays
    // q (AltGr q is @).
    layout.set_compose_key(Some(100));
    assert_eq!(typed(&mut layout, &[100, 30, 18, 16]), "æq");
    // Held when a layout takes over, it is not AltGr either.
    layout.hold(100);
    assert_eq!(typed(&mut layout, &[16]), "q");
    // A sequence that does not go on types its last key.
    assert_eq!(typed(&mut layout, &[100, 30, 2]), "1");
    // Without the rules, the compose key types nothing.
    let mut plain = Layout::from_keymap("de", keymap("de").as_bytes()).unwrap();
    plain.set_compose_key(Some(100));
    assert_eq!(typed(&mut plain, &[100, 30]), "a");
    // The decoder gives the key to the layouts it gets.
    let mut decoder = Decoder::new();
    decoder.set_compose_key(Some(100));
    decoder.set_layout(Some(german()));
    assert_eq!(decoder.feed(100, 1), None);
    assert_eq!(decoder.feed(100, 0), None);
    decoder.feed(30, 1);
    decoder.feed(30, 0);
    assert_eq!(decoder.feed(18, 1).unwrap().key, Key::Char('æ'));
    assert_eq!(crate::keys::compose_key("menu"), Some(127));
    assert_eq!(crate::keys::compose_key("pause"), None);
}

#[test]
fn compose_strings_are_quoted() {
    let rules = compile::dead_keys(b"<dead_acute> <quotedbl> : \"\\\"\"\n<a> : \"b\"\n").unwrap();
    assert_eq!(rules, "<dead_acute> <quotedbl> : \"\\\"\"\n");
    assert_eq!(single(Some("ab")), None);
    assert_eq!(single(None), None);
}

#[test]
fn control_keys_take_the_us_letter_on_other_scripts() {
    let russian = Layout::from_keymap("ru", keymap("ru").as_bytes()).unwrap();
    let mut decoder = Decoder::new();
    decoder.set_layout(Some(russian));

    assert_eq!(decoder.feed(37, 1).unwrap().key, Key::Char('л'));
    decoder.feed(29, 1);
    let control = decoder.feed(37, 1).unwrap();
    assert_eq!((control.key, control.ctrl), (Key::Char('k'), true));
    assert_eq!(decoder.feed(103, 1).unwrap().key, Key::Up);
}

#[test]
fn a_decoder_takes_a_layout_with_the_keys_held() {
    let mut decoder = Decoder::new();
    assert_eq!(decoder.layout(), "us");
    decoder.feed(42, 1);

    decoder.set_layout(Some(german()));

    assert_eq!(decoder.layout(), "de");
    assert_eq!(decoder.feed(3, 1).unwrap().key, Key::Char('"'));
    let tab = decoder.feed(15, 1).unwrap();
    assert_eq!((tab.key, tab.shift), (Key::BackTab, true));
    decoder.feed(42, 0);
    assert_eq!(decoder.feed(44, 1).unwrap().key, Key::Char('y'));
    assert_eq!(decoder.feed(41, 1), None);
    assert_eq!(decoder.feed(42, 1), None);
    decoder.set_layout(None);
    assert_eq!(decoder.feed(44, 1).unwrap().key, Key::Char('Z'));
}

#[test]
fn an_accent_that_does_not_combine_is_typed_before_the_key() {
    let mut decoder = Decoder::new();
    decoder.set_layout(Some(german()));
    let mut presses = Vec::new();

    for (code, value) in [(41, 1), (41, 0), (45, 1), (45, 0), (41, 1), (18, 1)] {
        decoder.feed_into(code, value, &mut presses);
    }

    let keys: Vec<Key> = presses.iter().map(|press| press.key).collect();
    assert_eq!(keys, [Key::Char('^'), Key::Char('x'), Key::Char('ê')]);
}

#[test]
fn caps_lock_of_a_layout_is_known() {
    let mut decoder = Decoder::new();
    decoder.set_layout(Some(german()));
    assert!(!decoder.is_caps_lock());

    decoder.feed(58, 1);
    decoder.feed(58, 0);

    assert!(decoder.is_caps_lock());
    assert_eq!(decoder.feed(30, 1).unwrap().key, Key::Char('A'));
}

#[test]
fn num_lock_turns_the_keypad_of_a_layout_into_digits() {
    let mut decoder = Decoder::new();
    decoder.set_layout(Some(german()));
    let key = |decoder: &mut Decoder, code| {
        let press = decoder.feed(code, 1).map(|press| press.key);
        decoder.feed(code, 0);
        press
    };

    assert!(!decoder.is_num_lock());
    assert_eq!(key(&mut decoder, 72), Some(Key::Up));
    assert_eq!(key(&mut decoder, 83), Some(Key::Delete));

    key(&mut decoder, crate::keys::NUM_LOCK);
    assert!(decoder.is_num_lock());
    assert_eq!(key(&mut decoder, 72), Some(Key::Char('8')));
    // The German keypad has a comma for its decimal point.
    assert_eq!(key(&mut decoder, 83), Some(Key::Char(',')));

    // Num Lock stays across a change of layout, both ways.
    decoder.set_layout(None);
    assert!(decoder.is_num_lock());
    decoder.set_layout(Some(german()));
    assert!(decoder.is_num_lock());
    decoder.set_num_lock(false);
    assert_eq!(key(&mut decoder, 79), Some(Key::End));
}

#[test]
fn the_accent_of_a_dead_key_waits_until_the_next_key() {
    let mut decoder = Decoder::new();
    decoder.set_layout(Some(german()));
    let mut presses = Vec::new();
    assert_eq!(decoder.pending_accent(), None);
    decoder.feed_into(41, 1, &mut presses);
    decoder.feed_into(41, 0, &mut presses);
    assert_eq!(decoder.pending_accent(), Some('^'));
    decoder.feed_into(18, 1, &mut presses);
    assert_eq!(decoder.pending_accent(), None);
    assert_eq!(presses.len(), 1);
    // The Super key of a layout is a modifier, as on the US one.
    decoder.feed_into(125, 1, &mut presses);
    decoder.feed_into(57, 1, &mut presses);
    assert!(presses.last().is_some_and(|press| press.logo));
}
