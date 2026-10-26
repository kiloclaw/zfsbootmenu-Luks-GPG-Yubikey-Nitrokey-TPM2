//! Key presses from Linux key codes, with a keyboard layout.
//!
//! The input devices report keys by position (`KEY_Q` is the key left of
//! `KEY_W`, whatever is printed on it). Without the kernel console, which
//! translates them with the image's keymap, the console on the display
//! translates them itself: with an XKB [`Layout`], or on the US layout when
//! it has none.

use crate::layout::Layout;

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

impl KeyPress {
    /// What wiped key presses are overwritten with.
    pub const BLANK: KeyPress = KeyPress {
        key: Key::Char('\0'),
        shift: false,
        ctrl: false,
        alt: false,
    };
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
    (74, '-', '-'),
    (78, '+', '+'),
    (98, '/', '/'),
];

/// The keys of the keypad that Num Lock turns into digits, by key code:
/// the digit, and the key they are without Num Lock (none for 5).
const KEYPAD: &[(u16, char, Option<Key>)] = &[
    (71, '7', Some(Key::Home)),
    (72, '8', Some(Key::Up)),
    (73, '9', Some(Key::PageUp)),
    (75, '4', Some(Key::Left)),
    (76, '5', None),
    (77, '6', Some(Key::Right)),
    (79, '1', Some(Key::End)),
    (80, '2', Some(Key::Down)),
    (81, '3', Some(Key::PageDown)),
    (82, '0', Some(Key::Insert)),
    (83, '.', Some(Key::Delete)),
];

/// The keypad key of `code`, if it is one: its digit, and its key without
/// Num Lock.
fn keypad(code: u16) -> Option<(char, Option<Key>)> {
    KEYPAD
        .iter()
        .find(|(c, ..)| *c == code)
        .map(|&(_, digit, key)| (digit, key))
}

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

/// The key of `code` if it types no character, on any layout.
fn special(code: u16, shift: bool) -> Option<Key> {
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
    None
}

/// `KEY_CAPSLOCK`.
const CAPS_LOCK: u16 = 58;

/// `KEY_NUMLOCK`.
pub const NUM_LOCK: u16 = 69;

/// How many keys can be down at once without the list of them growing,
/// which would leave a copy in freed memory.
const DOWN: usize = 32;

/// Removes `code` from `codes` without leaving a copy of the last code in
/// the spare capacity: the codes of the keys down tell a passphrase.
fn remove_code(codes: &mut Vec<u16>, code: u16) {
    if let Some(index) = codes.iter().position(|&down| down == code) {
        codes.remove(index);
        if let Some(spare) = codes.spare_capacity_mut().first_mut() {
            spare.write(0);
        }
    }
}

/// Turns key events into key presses, keeping the modifiers held and Caps
/// Lock.
#[derive(Debug)]
pub struct Decoder {
    /// The modifier keys held, by key code, on the US layout.
    held: Vec<u16>,
    caps_lock: bool,
    num_lock: bool,
    /// The layout, unless it is the US layout of this module's tables.
    layout: Option<Layout>,
    /// The keys down, by key code, which a new layout takes over.
    down: Vec<u16>,
    /// The accent typed before the last press, from a dead key.
    accent: Option<char>,
}

impl Default for Decoder {
    fn default() -> Decoder {
        Decoder {
            held: Vec::with_capacity(DOWN),
            caps_lock: false,
            num_lock: false,
            layout: None,
            down: Vec::with_capacity(DOWN),
            accent: None,
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        zbm_sys::wipe_values(&mut self.down, 0);
    }
}

impl Decoder {
    /// A decoder with nothing held.
    pub fn new() -> Decoder {
        Decoder::default()
    }

    /// Whether Caps Lock is on.
    pub fn is_caps_lock(&self) -> bool {
        self.layout
            .as_ref()
            .map_or(self.caps_lock, Layout::is_caps_lock)
    }

    /// Whether Num Lock is on: the keypad types digits.
    pub fn is_num_lock(&self) -> bool {
        self.layout
            .as_ref()
            .map_or(self.num_lock, Layout::is_num_lock)
    }

    /// Turns Num Lock on or off, as the keyboards' light showed it when the
    /// console took them over.
    pub fn set_num_lock(&mut self, on: bool) {
        self.num_lock = on;
        if let Some(layout) = &mut self.layout {
            layout.set_num_lock(on);
        }
    }

    /// Takes `now_down`, the keys down as the device reports them, as the
    /// keys held after events were lost: keys released meanwhile are
    /// released, modifiers and Caps Lock pressed meanwhile count.
    pub fn resync(&mut self, now_down: &[u16]) {
        let released: Vec<u16> = self
            .down
            .iter()
            .copied()
            .filter(|code| !now_down.contains(code))
            .collect();
        for code in released {
            let _ = self.feed(code, 0);
        }
        for &code in now_down {
            if self.down.contains(&code) {
                continue;
            }
            if let Some(layout) = &mut self.layout {
                layout.hold(code);
            } else if modifier(code).is_some() {
                self.held.push(code);
            } else if code == CAPS_LOCK {
                self.caps_lock = !self.caps_lock;
            } else if code == NUM_LOCK {
                self.num_lock = !self.num_lock;
            }
            self.down.push(code);
        }
    }

    /// Adds to `presses` what a key event types: key `code`, released (0),
    /// pressed (1) or repeated (2). An accent that the key does not
    /// combine with comes first, as on the kernel's keymap.
    pub fn feed_into(&mut self, code: u16, value: i32, presses: &mut Vec<KeyPress>) {
        if let Some(press) = self.feed(code, value) {
            if let Some(accent) = self.accent.take() {
                presses.push(KeyPress {
                    key: Key::Char(accent),
                    ..press
                });
            }
            presses.push(press);
        }
    }

    /// Translates keys with `layout` from now on, or on the US layout; the
    /// keys down stay down, and Num Lock as it was.
    pub fn set_layout(&mut self, layout: Option<Layout>) {
        let num_lock = self.is_num_lock();
        self.layout = layout;
        self.num_lock = num_lock;
        if let Some(layout) = &mut self.layout {
            for &code in &self.down {
                layout.hold(code);
            }
            layout.set_num_lock(num_lock);
        }
    }

    /// The name of the layout keys are translated with: `us` without one.
    pub fn layout(&self) -> &str {
        self.layout.as_ref().map_or("us", Layout::name)
    }

    fn is_held(&self, wanted: Modifier) -> bool {
        self.held.iter().any(|&code| modifier(code) == Some(wanted))
    }

    /// The press of a key event: key `code`, released (0), pressed (1) or
    /// repeated (2); `None` for releases and the modifiers themselves.
    /// An accent typed before the press is kept for [`Decoder::feed_into`].
    pub fn feed(&mut self, code: u16, value: i32) -> Option<KeyPress> {
        remove_code(&mut self.down, code);
        if value != 0 {
            self.down.push(code);
        }
        if let Some(layout) = &mut self.layout {
            let typed = layout.key(code, value)?;
            self.accent = typed.before;
            let num_lock = layout.is_num_lock();
            let key = match (special(code, typed.shift), typed.char) {
                (Some(key), _) => key,
                // The keypad: what the layout types with Num Lock (a comma
                // on some), the arrows and the rest without.
                (None, c) if keypad(code).is_some() => match (num_lock, c) {
                    (true, Some(c)) => Key::Char(c),
                    _ => keypad(code)?.1?,
                },
                // Ctrl and Alt with a key of a layout not in Latin letters
                // take the key's US letter, as `Ctrl-C` needs
                (None, Some(c)) if (typed.ctrl || typed.alt) && !c.is_ascii() => {
                    self.key(code, typed.shift)?
                }
                (None, c) => Key::Char(c?),
            };
            return Some(KeyPress {
                key,
                shift: typed.shift,
                ctrl: typed.ctrl,
                alt: typed.alt,
            });
        }
        if modifier(code).is_some() {
            remove_code(&mut self.held, code);
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
        if code == NUM_LOCK {
            self.num_lock ^= value == 1;
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
        if let Some(key) = special(code, shift) {
            return Some(key);
        }
        if let Some((digit, key)) = keypad(code) {
            return if self.num_lock {
                Some(Key::Char(digit))
            } else {
                key
            };
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
}
