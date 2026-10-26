//! The keys the console takes for itself, before the programs get the
//! rest: Super+Space switches to the image's next keyboard layout, and
//! Ctrl+Shift+U starts typing a character by its Unicode code point, in
//! hexadecimal, ended by Space or Enter (Escape cancels it), as in GTK.
//!
//! What is being typed is kept in a fixed array, wiped once the character
//! is sent or cancelled: at a passphrase prompt it is part of the
//! passphrase.

use zbm_display::keys::{Key, KeyPress};

/// How many hexadecimal digits a code point has at most.
const DIGITS: usize = 6;

/// What a key press does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Taken {
    /// The programs get it.
    Type(KeyPress),
    /// The keyboards switch to the image's next layout.
    NextLayout,
    /// The console took it.
    Nothing,
}

/// A code point being typed after Ctrl+Shift+U.
#[derive(Debug, Default)]
struct CodePoint {
    digits: [u8; DIGITS],
    len: usize,
}

impl CodePoint {
    /// The character of the digits typed, if they make one.
    fn char(&self) -> Option<char> {
        let digits = self.digits.get(..self.len)?;
        let value = digits.iter().try_fold(0_u32, |value, &digit| {
            Some(value * 16 + char::from(digit).to_digit(16)?)
        })?;
        (self.len > 0).then_some(char::from_u32(value)?)
    }
}

impl Drop for CodePoint {
    fn drop(&mut self) {
        zbm_sys::wipe(&mut self.digits);
        self.len = 0;
    }
}

/// The state of the keys the console takes for itself.
#[derive(Debug, Default)]
pub struct Typing {
    code_point: Option<CodePoint>,
}

/// Whether `press` is Ctrl+Shift+U.
fn is_unicode_key(press: KeyPress) -> bool {
    press.ctrl && press.shift && matches!(press.key, Key::Char('u' | 'U'))
}

/// The press of character `c`, without modifiers.
fn plain(c: char) -> KeyPress {
    KeyPress {
        key: Key::Char(c),
        ..KeyPress::BLANK
    }
}

impl Typing {
    /// What `press` does.
    pub fn take(&mut self, press: KeyPress) -> Taken {
        if let Some(code_point) = &mut self.code_point {
            return match press.key {
                Key::Char(c) if c.is_ascii_hexdigit() && !press.ctrl && !press.alt => {
                    if let Some(slot) = code_point.digits.get_mut(code_point.len) {
                        *slot = u8::try_from(c).unwrap_or(b'0');
                        code_point.len += 1;
                    }
                    Taken::Nothing
                }
                Key::Backspace if code_point.len > 0 => {
                    code_point.len -= 1;
                    Taken::Nothing
                }
                Key::Char(' ') | Key::Enter => {
                    let typed = code_point.char();
                    self.code_point = None;
                    typed.map_or(Taken::Nothing, |c| Taken::Type(plain(c)))
                }
                Key::Escape | Key::Backspace => {
                    self.code_point = None;
                    Taken::Nothing
                }
                _ => {
                    self.code_point = None;
                    Taken::Type(press)
                }
            };
        }
        if press.logo && press.key == Key::Char(' ') {
            Taken::NextLayout
        } else if is_unicode_key(press) {
            self.code_point = Some(CodePoint::default());
            Taken::Nothing
        } else {
            Taken::Type(press)
        }
    }

    /// What is being typed, to show at the cursor: `u` and the digits of a
    /// code point.
    pub fn preview(&self) -> Option<Preview> {
        let code_point = self.code_point.as_ref()?;
        let mut preview = Preview::of('u');
        for &digit in code_point.digits.get(..code_point.len)? {
            preview.push(char::from(digit));
        }
        Some(preview)
    }
}

/// What the console shows at the cursor while it is being typed: a dead
/// key's accent, or a code point.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Preview {
    chars: [char; 1 + DIGITS],
    len: usize,
}

impl Preview {
    /// A preview of `c`.
    pub fn of(c: char) -> Preview {
        let mut preview = Preview::default();
        preview.push(c);
        preview
    }

    fn push(&mut self, c: char) {
        if let Some(slot) = self.chars.get_mut(self.len) {
            *slot = c;
            self.len += 1;
        }
    }

    /// The characters shown.
    pub fn chars(&self) -> &[char] {
        self.chars.get(..self.len).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn press(key: Key) -> KeyPress {
        KeyPress {
            key,
            ..KeyPress::BLANK
        }
    }

    fn take_all(typing: &mut Typing, keys: &[KeyPress]) -> Vec<Taken> {
        keys.iter().map(|&key| typing.take(key)).collect()
    }

    const CTRL_SHIFT_U: KeyPress = KeyPress {
        key: Key::Char('U'),
        ctrl: true,
        shift: true,
        ..KeyPress::BLANK
    };

    #[test]
    fn ctrl_shift_u_types_a_code_point() {
        let mut typing = Typing::default();
        let keys = [
            CTRL_SHIFT_U,
            press(Key::Char('1')),
            press(Key::Char('f')),
            press(Key::Char('6')),
        ];
        let taken = take_all(&mut typing, &keys);
        assert!(taken.iter().all(|taken| *taken == Taken::Nothing));
        assert_eq!(typing.preview().unwrap().chars(), ['u', '1', 'f', '6']);
        assert_eq!(typing.take(press(Key::Char('0'))), Taken::Nothing);
        assert_eq!(
            typing.take(press(Key::Char(' '))),
            Taken::Type(plain('\u{1f60}'))
        );
        assert_eq!(typing.preview(), None);
        // Enter ends it too; Backspace takes a digit back.
        let keys = [
            CTRL_SHIFT_U,
            press(Key::Char('e')),
            press(Key::Char('9')),
            press(Key::Backspace),
            press(Key::Char('a')),
            press(Key::Enter),
        ];
        assert_eq!(
            take_all(&mut typing, &keys).last(),
            Some(&Taken::Type(plain('ê')))
        );
    }

    #[test]
    fn escape_or_another_key_cancels_a_code_point() {
        let mut typing = Typing::default();
        let x = press(Key::Char('x'));
        let taken = take_all(&mut typing, &[CTRL_SHIFT_U, press(Key::Char('4')), x]);
        assert_eq!(taken.last(), Some(&Taken::Type(x)));
        let keys = [CTRL_SHIFT_U, press(Key::Escape), x];
        assert_eq!(take_all(&mut typing, &keys).last(), Some(&Taken::Type(x)));
        // Nothing typed, or no character: nothing is sent.
        let keys = [CTRL_SHIFT_U, press(Key::Enter)];
        assert_eq!(take_all(&mut typing, &keys).last(), Some(&Taken::Nothing));
        let keys = [CTRL_SHIFT_U, press(Key::Char('d')), press(Key::Char('8'))]
            .into_iter()
            .chain([
                press(Key::Char('0')),
                press(Key::Char('0')),
                press(Key::Enter),
            ])
            .collect::<Vec<_>>();
        assert_eq!(take_all(&mut typing, &keys).last(), Some(&Taken::Nothing));
        // Seven digits: the seventh is not kept.
        let mut keys = vec![CTRL_SHIFT_U];
        keys.extend(std::iter::repeat_n(press(Key::Char('1')), 7));
        take_all(&mut typing, &keys);
        assert_eq!(typing.preview().unwrap().chars().len(), 7);
        // Backspace with no digit cancels.
        let keys = [CTRL_SHIFT_U, press(Key::Backspace)];
        take_all(&mut typing, &keys);
        assert_eq!(typing.preview(), None);
    }

    #[test]
    fn super_space_switches_layouts_and_other_keys_pass() {
        let mut typing = Typing::default();
        let super_space = KeyPress {
            logo: true,
            ..press(Key::Char(' '))
        };
        assert_eq!(typing.take(super_space), Taken::NextLayout);
        let a = press(Key::Char('a'));
        assert_eq!(typing.take(a), Taken::Type(a));
        assert_eq!(Preview::of('^').chars(), ['^']);
    }
}
