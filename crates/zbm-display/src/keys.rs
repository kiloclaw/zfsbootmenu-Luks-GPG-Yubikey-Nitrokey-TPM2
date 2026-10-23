//! Key presses from Linux key codes, on the US layout.
//!
//! The input devices report keys by position (`KEY_Q` is the key left of
//! `KEY_W`, whatever is printed on it). Without the kernel console, which
//! translates them with the image's keymap, the menu translates them itself.
//! The US layout is enough for the prototype; the menu's own layout switch
//! (`Ctrl-K` in the unlock dialog) works on the characters it gets, and real
//! layouts come with `xkbcommon` (ROADMAP.md #19, step 2).

/// A key, as the menu sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    /// A character, as the layout gives it with Shift and Caps Lock.
    Char(char),
    /// Enter, on the main keyboard or the keypad.
    Enter,
    /// Escape.
    Escape,
    /// Backspace.
    Backspace,
    /// Tab.
    Tab,
    /// Shift-Tab.
    BackTab,
    /// The arrow up.
    Up,
    /// The arrow down.
    Down,
    /// The arrow left.
    Left,
    /// The arrow right.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Insert.
    Insert,
    /// Delete.
    Delete,
    /// A function key, F1 to F12.
    F(u8),
}

/// A key pressed, with the modifiers held.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyPress {
    /// The key.
    pub key: Key,
    /// Whether Shift was held.
    pub shift: bool,
    /// Whether Ctrl was held.
    pub ctrl: bool,
    /// Whether Alt was held.
    pub alt: bool,
}

/// The characters of the keys that type one, by key code: unshifted and
/// shifted.
const CHARACTERS: &[(u16, char, char)] = &[
    (2, '1', '!'),
    (3, '2', '@'),
    (4, '3', '#'),
    (5, '4', '$'),
    (6, '5', '%'),
    (7, '6', '^'),
    (8, '7', '&'),
    (9, '8', '*'),
    (10, '9', '('),
    (11, '0', ')'),
    (12, '-', '_'),
    (13, '=', '+'),
    (26, '[', '{'),
    (27, ']', '}'),
    (39, ';', ':'),
    (40, '\'', '"'),
    (41, '`', '~'),
    (43, '\\', '|'),
    (51, ',', '<'),
    (52, '.', '>'),
    (53, '/', '?'),
    (55, '*', '*'),
    (57, ' ', ' '),
];

/// The letters, by key code, in the rows of the keyboard.
const LETTERS: &[(u16, &str)] = &[(16, "qwertyuiop"), (30, "asdfghjkl"), (44, "zxcvbnm")];

/// The keys that type nothing, by key code.
const SPECIAL: &[(u16, Key)] = &[
    (1, Key::Escape),
    (14, Key::Backspace),
    (15, Key::Tab),
    (28, Key::Enter),
    (96, Key::Enter),
    (102, Key::Home),
    (103, Key::Up),
    (104, Key::PageUp),
    (105, Key::Left),
    (106, Key::Right),
    (107, Key::End),
    (108, Key::Down),
    (109, Key::PageDown),
    (110, Key::Insert),
    (111, Key::Delete),
    (87, Key::F(11)),
    (88, Key::F(12)),
];

/// The modifier keys: left and right Shift, Ctrl and Alt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Modifier {
    Shift,
    Ctrl,
    Alt,
}

/// The modifier of key `code`, if it is one.
fn modifier(code: u16) -> Option<Modifier> {
    match code {
        42 | 54 => Some(Modifier::Shift),
        29 | 97 => Some(Modifier::Ctrl),
        56 | 100 => Some(Modifier::Alt),
        _ => None,
    }
}

/// `KEY_CAPSLOCK`.
const CAPS_LOCK: u16 = 58;

/// Turns key events into key presses, keeping the modifiers held and Caps
/// Lock.
#[derive(Clone, Debug, Default)]
pub struct Decoder {
    /// The modifier keys held, by key code.
    held: Vec<u16>,
    caps_lock: bool,
}

impl Decoder {
    /// A decoder with nothing held.
    pub fn new() -> Decoder {
        Decoder::default()
    }

    fn is_held(&self, wanted: Modifier) -> bool {
        self.held.iter().any(|&code| modifier(code) == Some(wanted))
    }

    /// The press of a key event: key `code`, released (0), pressed (1) or
    /// repeated (2); `None` for releases and the modifiers themselves.
    pub fn feed(&mut self, code: u16, value: i32) -> Option<KeyPress> {
        if modifier(code).is_some() {
            self.held.retain(|&held| held != code);
            if value != 0 {
                self.held.push(code);
            }
            return None;
        }
        if value == 0 {
            return None;
        }
        if code == CAPS_LOCK {
            self.caps_lock ^= value == 1;
            return None;
        }
        let shift = self.is_held(Modifier::Shift);
        let key = self.key(code, shift)?;
        Some(KeyPress {
            key,
            shift,
            ctrl: self.is_held(Modifier::Ctrl),
            alt: self.is_held(Modifier::Alt),
        })
    }

    /// The key of `code` on the US layout.
    fn key(&self, code: u16, shift: bool) -> Option<Key> {
        if let Some(&(_, key)) = SPECIAL.iter().find(|(c, _)| *c == code) {
            return Some(if key == Key::Tab && shift {
                Key::BackTab
            } else {
                key
            });
        }
        if (59..=68).contains(&code) {
            return u8::try_from(code - 58).ok().map(Key::F);
        }
        if let Some(&(_, plain, shifted)) = CHARACTERS.iter().find(|(c, ..)| *c == code) {
            return Some(Key::Char(if shift { shifted } else { plain }));
        }
        let letter = LETTERS.iter().find_map(|&(first, row)| {
            let index = usize::from(code.checked_sub(first)?);
            row.chars().nth(index)
        })?;
        let upper = shift != self.caps_lock;
        Some(Key::Char(if upper {
            letter.to_ascii_uppercase()
        } else {
            letter
        }))
    }
}

#[cfg(test)]
mod tests {
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
}
