use pretty_assertions::assert_eq;

use super::*;

fn press(decoder: &mut Decoder, code: u16) -> Option<KeyPress> {
    decoder.feed(code, 1)
}

fn plain(key: Key) -> KeyPress {
    KeyPress {
        key,
        shift: false,
        ctrl: false,
        alt: false,
        logo: false,
        keypad: false,
    }
}

#[test]
fn keys_type_their_characters_on_the_us_layout() {
    let mut decoder = Decoder::new();
    let typed: String = [16, 17, 30, 44, 50, 2, 11, 12, 53, 57]
        .into_iter()
        .filter_map(|code| match press(&mut decoder, code)?.key {
            Key::Char(c) => Some(c),
            _ => None,
        })
        .collect();

    assert_eq!(typed, "qwazm10-/ ");
    assert_eq!(press(&mut decoder, 1), Some(plain(Key::Escape)));
    assert_eq!(press(&mut decoder, 28), Some(plain(Key::Enter)));
    assert_eq!(press(&mut decoder, 108), Some(plain(Key::Down)));
    assert_eq!(press(&mut decoder, 59), Some(plain(Key::F(1))));
    assert_eq!(press(&mut decoder, 88), Some(plain(Key::F(12))));
    assert_eq!(press(&mut decoder, 240), None);
    assert_eq!(press(&mut decoder, 26 + 30), None);
}

#[test]
fn shift_caps_lock_ctrl_and_alt_change_the_press() {
    let mut decoder = Decoder::new();
    decoder.feed(42, 1);

    assert_eq!(press(&mut decoder, 30).unwrap().key, Key::Char('A'));
    assert_eq!(press(&mut decoder, 2).unwrap().key, Key::Char('!'));
    assert_eq!(press(&mut decoder, 15).unwrap().key, Key::BackTab);
    decoder.feed(42, 0);
    decoder.feed(CAPS_LOCK, 1);
    decoder.feed(CAPS_LOCK, 2);
    assert_eq!(press(&mut decoder, 30).unwrap().key, Key::Char('A'));
    assert_eq!(press(&mut decoder, 2).unwrap().key, Key::Char('1'));
    decoder.feed(CAPS_LOCK, 1);

    decoder.feed(97, 1);
    decoder.feed(56, 1);
    let combined = press(&mut decoder, 37).unwrap();
    assert_eq!(combined.key, Key::Char('k'));
    assert!(combined.ctrl && combined.alt && !combined.shift);
    decoder.feed(97, 0);
    decoder.feed(56, 0);
    assert_eq!(decoder.feed(37, 2), Some(plain(Key::Char('k'))));
    assert_eq!(decoder.feed(37, 0), None);
}

#[test]
fn keys_lost_with_dropped_events_are_resynchronised() {
    let mut decoder = Decoder::new();
    decoder.feed(42, 1);
    decoder.feed(30, 1);

    // Shift and A were released while events were lost, Ctrl and Caps
    // Lock pressed.
    decoder.resync(&[29, CAPS_LOCK]);

    let press = press(&mut decoder, 48).unwrap();
    assert_eq!(press.key, Key::Char('B'));
    assert!(press.ctrl && !press.shift);
    assert!(decoder.is_caps_lock());
    decoder.resync(&[29, CAPS_LOCK]);
    assert!(decoder.is_caps_lock());
}

#[test]
fn released_keys_are_removed_in_place() {
    let mut codes = Vec::with_capacity(4);
    codes.extend([5, 6, 7]);
    let allocation = codes.as_ptr();

    remove_code(&mut codes, 5);
    remove_code(&mut codes, 9);

    assert_eq!(codes, [6, 7]);
    assert_eq!(codes.as_ptr(), allocation);
    assert_eq!(Decoder::new().down.capacity(), DOWN);
}

#[test]
fn the_keypad_types_digits_with_num_lock_and_moves_without() {
    let mut decoder = Decoder::new();
    let key = |decoder: &mut Decoder, code| press(decoder, code).map(|press| press.key);

    assert_eq!(key(&mut decoder, 71), Some(Key::Home));
    assert_eq!(key(&mut decoder, 83), Some(Key::Delete));
    assert_eq!(key(&mut decoder, 76), None);
    assert_eq!(key(&mut decoder, 98), Some(Key::Char('/')));
    assert_eq!(key(&mut decoder, 74), Some(Key::Char('-')));
    assert!(!decoder.is_num_lock());

    decoder.feed(NUM_LOCK, 1);
    decoder.feed(NUM_LOCK, 0);
    assert!(decoder.is_num_lock());
    assert_eq!(key(&mut decoder, 71), Some(Key::Char('7')));
    assert_eq!(key(&mut decoder, 76), Some(Key::Char('5')));
    assert_eq!(key(&mut decoder, 83), Some(Key::Char('.')));
    assert_eq!(key(&mut decoder, 78), Some(Key::Char('+')));
    assert!(press(&mut decoder, 78).unwrap().keypad);
    assert!(press(&mut decoder, 96).unwrap().keypad);
    assert!(!press(&mut decoder, 28).unwrap().keypad);
    assert!(!press(&mut decoder, 102).unwrap().keypad);

    decoder.set_num_lock(false);
    assert_eq!(key(&mut decoder, 82), Some(Key::Insert));
    // Num Lock pressed while events were lost counts.
    decoder.resync(&[NUM_LOCK]);
    assert!(decoder.is_num_lock());
}

#[test]
fn the_us_layout_has_no_accents_to_type_first() {
    let mut decoder = Decoder::new();
    let mut presses = Vec::new();

    decoder.feed_into(30, 1, &mut presses);
    decoder.feed_into(30, 0, &mut presses);

    assert_eq!(presses, [plain(Key::Char('a'))]);
    assert_eq!(KeyPress::BLANK.key, Key::Char('\0'));
}

#[test]
fn more_keys_and_the_logo_key() {
    let mut decoder = Decoder::new();
    let keys: Vec<Key> = [183, 194, 99, 119, 127, 113, 164]
        .into_iter()
        .filter_map(|code| press(&mut decoder, code))
        .map(|press| press.key)
        .collect();
    assert_eq!(
        keys,
        [
            Key::F(13),
            Key::F(24),
            Key::Print,
            Key::Pause,
            Key::Menu,
            Key::Media(Media::Mute),
            Key::Media(Media::PlayPause),
        ]
    );
    // The logo key is a modifier.
    assert_eq!(press(&mut decoder, 125), None);
    let with_logo = press(&mut decoder, 57).unwrap();
    assert!(with_logo.logo && with_logo.key == Key::Char(' '));
    decoder.feed(125, 0);
    assert!(!press(&mut decoder, 57).unwrap().logo);
}

#[test]
fn scroll_lock_is_a_lock_on_every_layout() {
    let mut decoder = Decoder::new();
    assert!(!decoder.is_scroll_lock());
    assert_eq!(press(&mut decoder, 70), None);
    decoder.feed(70, 0);
    assert!(decoder.is_scroll_lock());
    // Held, it repeats without turning back.
    decoder.feed(70, 1);
    decoder.feed(70, 2);
    assert!(!decoder.is_scroll_lock());
    decoder.set_layout(Some(crate::layout::tests::german()));
    decoder.feed(70, 1);
    assert!(decoder.is_scroll_lock());
}
