//! Key presses decoded from the bytes a terminal sends: Shift with
//! capitals, Ctrl with the control characters, Alt with Escape before a
//! key, and the sequences of the Linux console and of xterm for the keys
//! that type nothing. The menu reads its terminal's keys with it, and the
//! console the keys of a serial terminal (ROADMAP.md UI9); what is decoded
//! may be a passphrase, so callers wipe the bytes and the presses.

use std::time::Duration;

use crate::keys::{Key, KeyPress};

/// The modifiers of a decoded key, as bits: Shift, Ctrl, Alt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Modifiers(u8);

impl Modifiers {
    /// None.
    const NONE: Modifiers = Modifiers(0);
    /// Shift.
    const SHIFT: Modifiers = Modifiers(1);
    /// Ctrl.
    const CTRL: Modifiers = Modifiers(2);
    /// Alt.
    const ALT: Modifiers = Modifiers(4);

    /// Whether `other` is among them.
    fn contains(self, other: Modifiers) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Modifiers;

    fn bitor(self, other: Modifiers) -> Modifiers {
        Modifiers(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, other: Modifiers) {
        self.0 |= other.0;
    }
}

/// `key` pressed with `modifiers`.
fn press(key: Key, modifiers: Modifiers) -> KeyPress {
    KeyPress {
        shift: modifiers.contains(Modifiers::SHIFT),
        ctrl: modifiers.contains(Modifiers::CTRL),
        alt: modifiers.contains(Modifiers::ALT),
        ..KeyPress::new(key)
    }
}

/// `ESC`.
const ESCAPE: u8 = 0x1b;

/// How long the rest of a key's sequence may take to come after its last
/// byte on a virtual console, where the kernel writes the bytes of a key at
/// once; then `ESC` alone is the Escape key.
pub const CONSOLE_ESCAPE_WAIT: Duration = Duration::from_millis(25);

/// The same on other terminals, such as a serial console reached through
/// the network, which may deliver a sequence in pieces. An arrow key read
/// as Escape there would skip the volume of an unlock dialog.
pub const SERIAL_ESCAPE_WAIT: Duration = Duration::from_millis(100);

/// A key press decoded, or `None` for bytes that name no key, and how many
/// bytes it took.
type Decoded = (Option<KeyPress>, usize);

/// A key with no modifier.
fn plain(code: Key) -> KeyPress {
    press(code, Modifiers::NONE)
}

/// The key a control character or Delete sends, if `byte` is one.
fn control(byte: u8) -> Option<KeyPress> {
    let (code, modifiers) = match byte {
        b'\r' | b'\n' => (Key::Enter, Modifiers::NONE),
        b'\t' => (Key::Tab, Modifiers::NONE),
        0x7f | 0x08 => (Key::Backspace, Modifiers::NONE),
        0x01..=0x1a => (Key::Char(char::from(byte - 1 + b'a')), Modifiers::CTRL),
        0x1c..=0x1f => (Key::Char(char::from(byte - 0x1c + b'4')), Modifiers::CTRL),
        0 => (Key::Char(' '), Modifiers::CTRL),
        _ => return None,
    };
    Some(press(code, modifiers))
}

/// The character at the front of `bytes`, Shift with a capital.
fn character(bytes: &[u8], last: bool) -> Option<Decoded> {
    let width = match bytes.first()? {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => return Some((None, 1)),
    };
    // A byte that cannot go on the character ends it: the lead byte alone
    // is dropped, and what follows, such as Enter, is a key of its own.
    let continued = bytes
        .iter()
        .skip(1)
        .take(width - 1)
        .take_while(|&&byte| byte & 0xc0 == 0x80)
        .count();
    let Some(encoded) = bytes.get(..width).filter(|_| continued == width - 1) else {
        let cut = continued + 1 < bytes.len().min(width);
        return (cut || last).then_some((None, 1));
    };
    let key = std::str::from_utf8(encoded)
        .ok()
        .and_then(|text| text.chars().next())
        // A C1 control, such as U+009B, types nothing.
        .filter(|c| !c.is_control())
        .map(|c| {
            let shift = if c.is_uppercase() {
                Modifiers::SHIFT
            } else {
                Modifiers::NONE
            };
            press(Key::Char(c), shift)
        });
    Some((key, width))
}

/// A sequence that is not whole: the Escape key when nothing more comes,
/// the rest dropped.
fn unfinished(bytes: &[u8], last: bool) -> Option<Decoded> {
    last.then_some((Some(plain(Key::Escape)), bytes.len()))
}

/// The modifiers of the parameter `value` of a sequence: 1 plus Shift (1),
/// Alt (2) and Ctrl (4).
fn modifiers(value: u8) -> Modifiers {
    let mask = value.saturating_sub(1);
    let mut modifiers = Modifiers::NONE;
    for (bit, modifier) in [
        (1, Modifiers::SHIFT),
        (2, Modifiers::ALT),
        (4, Modifiers::CTRL),
    ] {
        if mask & bit != 0 {
            modifiers |= modifier;
        }
    }
    modifiers
}

/// The key of `ESC [ number ~`.
fn numbered(number: u8) -> Option<Key> {
    Some(match number {
        1 | 7 => Key::Home,
        2 => Key::Insert,
        3 => Key::Delete,
        4 | 8 => Key::End,
        5 => Key::PageUp,
        6 => Key::PageDown,
        11..=15 => Key::F(number - 10),
        17..=21 => Key::F(number - 11),
        23..=24 => Key::F(number - 12),
        _ => return None,
    })
}

/// The key of the control sequence `ESC [ parameters final`.
fn sequence_key(last: u8, parameters: &[u8]) -> Option<KeyPress> {
    let text = std::str::from_utf8(parameters).ok()?;
    let mut values = text.split(';').map(|value| value.parse().unwrap_or(1));
    let first = values.next().unwrap_or(1);
    let modifiers = modifiers(values.next().unwrap_or(1));
    let code = match last {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        b'Z' => return Some(press(Key::BackTab, Modifiers::SHIFT)),
        b'~' => numbered(first)?,
        _ => return None,
    };
    Some(press(code, modifiers))
}

/// The control sequence at the front of `bytes`, which start `ESC [`; the
/// Linux console sends `ESC [ [ letter` for F1 to F5.
fn control_sequence(bytes: &[u8], last: bool) -> Option<Decoded> {
    let body = bytes.get(2..).unwrap_or_default();
    if body.first() == Some(&b'[') {
        let Some(&letter) = body.get(1) else {
            return unfinished(bytes, last);
        };
        if !is_final(letter) {
            return Some((None, 3));
        }
        let key = matches!(letter, b'A'..=b'E').then(|| plain(Key::F(1 + letter - b'A')));
        return Some((key, 4));
    }
    // A control character ends a sequence that is broken: the sequence is
    // dropped, and the control character is a key of its own.
    if let Some(broken) = body.iter().position(|&byte| byte < 0x20 || byte == 0x7f)
        && !body.iter().take(broken).any(|&byte| is_final(byte))
    {
        return Some((None, 2 + broken));
    }
    let Some(end) = body.iter().position(|&byte| is_final(byte)) else {
        // Without its end, it is not waited for without limit either.
        if bytes.len() >= STRING_MAX {
            return Some((None, bytes.len()));
        }
        return unfinished(bytes, last);
    };
    let (parameters, rest) = body.split_at(end);
    let key = rest.first().and_then(|&end| sequence_key(end, parameters));
    Some((key, 2 + end + 1))
}

/// Whether `byte` ends a control sequence (`0x40` to `0x7e`).
fn is_final(byte: u8) -> bool {
    (0x40..=0x7e).contains(&byte)
}

/// The sequence `ESC O letter` at the front of `bytes`; Alt+O when what
/// follows cannot end it, so that a key typed after it, such as Enter, is
/// not lost.
fn single_shift(bytes: &[u8], last: bool) -> Option<Decoded> {
    let Some(&letter) = bytes.get(2) else {
        return unfinished(bytes, last);
    };
    if !is_final(letter) {
        return alt(bytes, last);
    }
    let code = match letter {
        b'A' => Some(Key::Up),
        b'B' => Some(Key::Down),
        b'C' => Some(Key::Right),
        b'D' => Some(Key::Left),
        b'H' => Some(Key::Home),
        b'F' => Some(Key::End),
        b'P'..=b'S' => Some(Key::F(1 + letter - b'P')),
        _ => None,
    };
    Some((code.map(plain), 3))
}

/// The longest sequence waited for: one that is longer without its end is
/// dropped as far as it came, so that it cannot hold the keys back.
const STRING_MAX: usize = 4096;

/// The string sequence at the front of `bytes`, which start `ESC ]` (OSC),
/// `ESC P` (DCS), `ESC _` (APC), `ESC ^` (PM) or `ESC X` (SOS), up to its
/// end, BEL or `ESC \` (ST). Nobody types one: a terminal sends them as
/// replies, such as its colours (`OSC 11`), which a program printed a query
/// for. Such a reply names no key; read as keys, it would type its text.
/// Without its end when nothing more comes, it is Alt and the key after
/// `ESC`, as typed; `None` while its end may still come.
fn string_sequence(bytes: &[u8], last: bool) -> Option<Decoded> {
    let body = bytes.get(2..).unwrap_or_default();
    let end = body.iter().enumerate().find_map(|(at, &byte)| match byte {
        0x07 => Some(at + 1),
        ESCAPE if body.get(at + 1) == Some(&b'\\') => Some(at + 2),
        _ => None,
    });
    match end {
        Some(end) => Some((None, 2 + end)),
        None if bytes.len() >= STRING_MAX => Some((None, bytes.len())),
        None if last => alt(bytes, last),
        None => None,
    }
}

/// Alt with the key after the `ESC` at the front of `bytes`.
fn alt(bytes: &[u8], last: bool) -> Option<Decoded> {
    let (key, length) = next(bytes.get(1..).unwrap_or_default(), last)?;
    Some((key.map(KeyPress::with_alt), length + 1))
}

/// What starts with `ESC` at the front of `bytes`: a sequence, Alt with the
/// key after it, or the Escape key.
fn escape(bytes: &[u8], last: bool) -> Option<Decoded> {
    let Some(&second) = bytes.get(1) else {
        return unfinished(bytes, last);
    };
    match second {
        b'[' => control_sequence(bytes, last),
        b'O' => single_shift(bytes, last),
        b']' | b'P' | b'_' | b'^' | b'X' => string_sequence(bytes, last),
        ESCAPE => Some((Some(plain(Key::Escape)), 1)),
        _ => alt(bytes, last),
    }
}

/// The key press at the front of `bytes`; `None` when they do not make a
/// whole one yet, unless `last`: no more bytes come.
fn next(bytes: &[u8], last: bool) -> Option<Decoded> {
    let first = *bytes.first()?;
    if first == ESCAPE {
        return escape(bytes, last);
    }
    // A terminal that sends CR LF for Enter (telnet, some console servers)
    // presses it once.
    if bytes.starts_with(b"\r\n") {
        return Some((Some(plain(Key::Enter)), 2));
    }
    match control(first) {
        Some(key) => Some((Some(key), 1)),
        None => character(bytes, last),
    }
}

/// Whether the last key of `keys`, decoded from `bytes`, is an Enter that a
/// carriage return ended, with Alt or without: a line feed right after it,
/// in the next read, is the same Enter (CR LF), and is dropped.
pub fn ends_with_carriage_return(bytes: &[u8], keys: &[KeyPress]) -> bool {
    bytes.last() == Some(&b'\r') && keys.last().is_some_and(|press| press.key == Key::Enter)
}

/// Decodes the key presses in `bytes` into `keys`, and says how many bytes
/// they took: the bytes of a press that is not whole yet stay, unless
/// `last`. Bytes that name no key are dropped.
pub fn decode(bytes: &[u8], last: bool, keys: &mut Vec<KeyPress>) -> usize {
    let mut used = 0;
    while let Some((key, length)) = next(bytes.get(used..).unwrap_or_default(), last) {
        keys.extend(key);
        used += length;
    }
    used
}

#[cfg(test)]
mod tests;
