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

/// The positions of the layout `name`, with its dead keys.
fn positions(name: &str) -> Positions {
    Layout::from_keymap(name, keymap(name).as_bytes())
        .unwrap()
        .with_dead_keys(dead_keys().as_bytes())
        .unwrap()
        .positions()
}

#[test]
fn keys_translate_by_their_position_between_layouts() {
    let us = positions("us");
    let typed = |to: &Positions, text: &str| -> String {
        text.chars().map(|c| us.translate(to, c)).collect()
    };

    // The keys a German user presses for "Zürich-2" on a US console.
    assert_eq!(typed(&positions("de"), "Y[rich/@"), "Zürich-\"");
    assert_eq!(typed(&positions("fr"), "qzm2"), "aw,é");
    // The British layout: Shift-2 is the quote, and the key above Enter `#`.
    assert_eq!(typed(&positions("gb"), "@\\"), "\"#");
    // A variant: the key left of 1 is a dead circumflex in `de`, which
    // types `^` alone, and `^` itself in `de(nodeadkeys)`.
    assert_eq!(typed(&positions("de"), "`"), "^");
    assert_eq!(typed(&positions("de(nodeadkeys)"), "`"), "^");
    // Back to the console's, and what no key of the main block types.
    assert_eq!(positions("de").translate(&us, 'z'), 'y');
    assert_eq!(us.translate(&us, 'y'), 'y');
    assert_eq!(us.translate(&positions("de"), '€'), '€');
    assert_eq!(us.translate(&Positions::default(), 'y'), 'y');
}

#[test]
fn alt_takes_the_us_letter_on_other_scripts() {
    let russian = Layout::from_keymap("ru", keymap("ru").as_bytes()).unwrap();
    let mut decoder = Decoder::new();
    decoder.set_layout(Some(russian));

    decoder.feed(56, 1);
    let alt = decoder.feed(37, 1).unwrap();
    assert_eq!((alt.key, alt.alt, alt.ctrl), (Key::Char('k'), true, false));
}

#[test]
fn the_us_positions_are_the_decoder_s_and_xkb_s() {
    let us = Positions::us();
    let xkb = positions("us");
    let mut decoder = Decoder::new();
    for &code in MAIN_BLOCK.iter().filter(|&&code| code != 86) {
        for shift in [false, true] {
            if shift {
                decoder.feed(42, 1);
            }
            let Some(Key::Char(c)) = decoder.feed(code, 1).map(|press| press.key) else {
                panic!("the decoder types nothing on key {code}");
            };
            decoder.feed(code, 0);
            decoder.feed(42, 0);
            assert_eq!(us.key_of(c), Some((code, shift)), "{c}");
            // XKB's same key types it at the same level.
            assert_eq!(us.translate(&xkb, c), c, "{c}");
        }
    }
    assert_eq!(xkb.key_of('q'), Some((16, false)));
    assert_eq!(xkb.key_of('?'), Some((53, true)));
    // XKB's key 86 types `<` and `>` too, and comes first in the block;
    // the decoder's types nothing.
    assert_eq!(xkb.key_of('<'), Some((86, false)));
    assert_eq!(us.key_of('<'), Some((51, true)));
    assert_eq!(xkb.translate(&us, '<'), '<');
}

#[test]
fn a_character_names_its_key_and_level() {
    let french = positions("fr");
    assert_eq!(french.key_of('é'), Some((3, false)));
    assert_eq!(french.key_of('2'), Some((3, true)));
    // A dead key names the accent it types alone.
    assert_eq!(french.key_of('^'), Some((26, false)));
    // The third level (AltGr) is not kept, and nor is a composed letter.
    assert_eq!(french.key_of('€'), None);
    assert_eq!(french.key_of('ê'), None);
    // Two keys type `/` in Russian and `<` in Dvorak: the first in the
    // block names it.
    assert_eq!(positions("ru").key_of('/'), Some((43, true)));
    assert_eq!(positions("us(dvorak)").key_of('<'), Some((17, true)));
    assert_eq!(Positions::default().key_of('a'), None);
    // And a key names its characters.
    assert_eq!(french.character_of(3, false), Some('é'));
    assert_eq!(french.character_of(3, true), Some('2'));
    assert_eq!(french.character_of(1, false), None);
}

/// The layouts of the console's image of the VM tests, in its order
/// (`zbm-vmtest`'s `scenarios/display.rs`): `menu-keys` switches between
/// them.
const CONSOLE_IMAGE: [&str; 6] = ["us", "de", "fr", "gb", "ru", "us(dvorak)"];

#[test]
fn every_layout_of_the_console_image_compiles_and_types_what_menu_keys_expects() {
    // The keys `menu-keys` presses on each layout, by code, with Shift and
    // Caps Lock or not, and what they type there.
    let keys: [(&str, u16, bool, bool, char); 28] = [
        ("us", 3, false, false, '2'),
        ("us", 2, false, false, '1'),
        ("us", 38, false, false, 'l'),
        ("us", 53, true, false, '?'),
        ("de", 3, false, false, '2'),
        ("de", 2, false, false, '1'),
        ("de", 38, false, false, 'l'),
        ("de", 53, true, false, '_'),
        ("fr", 3, false, false, 'é'),
        ("fr", 2, false, false, '&'),
        ("fr", 53, false, false, '!'),
        ("fr", 53, true, false, '§'),
        ("fr", 3, false, true, 'É'),
        ("fr", 2, false, true, '&'),
        ("gb", 3, false, false, '2'),
        ("gb", 2, false, false, '1'),
        ("gb", 38, false, false, 'l'),
        ("gb", 53, true, false, '?'),
        ("ru", 38, false, false, 'д'),
        ("ru", 53, true, false, ','),
        ("ru", 46, false, false, 'с'),
        ("ru", 38, false, true, 'Д'),
        ("ru", 53, true, true, ','),
        ("ru", 3, false, false, '2'),
        ("ru", 2, false, false, '1'),
        ("us(dvorak)", 25, false, false, 'l'),
        ("us(dvorak)", 38, false, false, 'n'),
        ("us(dvorak)", 26, true, false, '?'),
    ];
    for name in CONSOLE_IMAGE {
        // A name the image's keymap file is named after, as the console
        // loads it; and a layout with a character on every key.
        assert!(compile::split(name).is_ok(), "{name}");
        let positions = positions(name);
        let on_every_key = MAIN_BLOCK
            .iter()
            .filter(|&&code| code != 86)
            .all(|&code| positions.character_of(code, false).is_some());
        assert!(on_every_key, "{name}");
    }
    for (name, code, shift, caps_lock, wanted) in keys {
        let layout = Layout::from_keymap(name, keymap(name).as_bytes())
            .and_then(|layout| layout.with_dead_keys(dead_keys().as_bytes()))
            .unwrap();
        let mut decoder = Decoder::new();
        decoder.set_layout(Some(layout));
        if caps_lock {
            decoder.feed(58, 1);
            decoder.feed(58, 0);
        }
        if shift {
            decoder.feed(42, 1);
        }
        let typed = decoder.feed(code, 1).map(|press| press.key);
        assert_eq!(typed, Some(Key::Char(wanted)), "{name}: {code}");
    }
}
