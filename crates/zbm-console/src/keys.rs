//! The bytes a key press sends to the program in the terminal, as the
//! Linux console sends them with its default keymap: characters, control
//! characters with Ctrl, Escape before them with Alt, and its escape
//! sequences for the keys that type nothing. The programs in the terminal
//! run with `TERM=linux`, as on the kernel console, and read keys through
//! that terminal description.
//!
//! What is typed may be a passphrase: the bytes are written into a buffer
//! the caller wipes once they are sent (`zbm_sys::wipe_vec`).

use zbm_display::keys::{Key, KeyPress};

/// `ESC`.
const ESCAPE: u8 = 0x1b;

/// The control character Ctrl gives with `c`, if any.
fn control(c: char) -> Option<u8> {
    let byte = u8::try_from(c).ok()?;
    match byte {
        b'a'..=b'z' => Some(byte - b'a' + 1),
        b'@' | b' ' | b'2' => Some(0),
        b'[' | b'3' => Some(ESCAPE),
        b'\\' | b'4' => Some(0x1c),
        b']' | b'5' => Some(0x1d),
        b'^' | b'6' => Some(0x1e),
        b'_' | b'7' | b'/' => Some(0x1f),
        b'?' | b'8' => Some(0x7f),
        b'A'..=b'Z' => Some(byte - b'A' + 1),
        _ => None,
    }
}

/// The final character of the cursor keys.
fn cursor_final(key: Key) -> Option<u8> {
    Some(match key {
        Key::Up => b'A',
        Key::Down => b'B',
        Key::Right => b'C',
        Key::Left => b'D',
        _ => return None,
    })
}

/// The sequence of the keys that type nothing other than the cursor keys:
/// `ESC [ [ letter` for F1 to F5, `ESC [ number ~` for the others.
fn other_sequence(key: Key) -> Option<String> {
    let number = match key {
        Key::F(number @ 1..=5) => {
            let letter = char::from(b'A' + number - 1);
            return Some(format!("\x1b[[{letter}"));
        }
        Key::Home => 1,
        Key::Insert => 2,
        Key::Delete => 3,
        Key::End => 4,
        Key::PageUp => 5,
        Key::PageDown => 6,
        Key::F(number @ 6..=10) => number + 11,
        Key::F(number @ 11..=12) => number + 12,
        _ => return None,
    };
    Some(format!("\x1b[{number}~"))
}

/// Appends a character with Ctrl and Alt applied.
fn character(press: KeyPress, c: char, out: &mut Vec<u8>) {
    if press.alt {
        out.push(ESCAPE);
    }
    if press.ctrl
        && let Some(byte) = control(c)
    {
        out.push(byte);
        return;
    }
    let mut utf8 = [0_u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut utf8).as_bytes());
}

/// Appends the bytes of `press` to `out`; `application_cursor` is the
/// terminal's cursor key mode (DECCKM), which full-screen programs set.
pub fn encode(press: KeyPress, application_cursor: bool, out: &mut Vec<u8>) {
    let alt = |out: &mut Vec<u8>| {
        if press.alt {
            out.push(ESCAPE);
        }
    };
    match press.key {
        Key::Char(c) => character(press, c, out),
        Key::Enter => {
            alt(out);
            out.push(b'\r');
        }
        Key::Escape => {
            alt(out);
            out.push(ESCAPE);
        }
        Key::Backspace => {
            alt(out);
            out.push(if press.ctrl { 0x08 } else { 0x7f });
        }
        Key::Tab => {
            alt(out);
            out.push(b'\t');
        }
        Key::BackTab => out.extend_from_slice(b"\x1b[Z"),
        key => sequence(key, application_cursor, out),
    }
}

/// Appends the escape sequence of a key that types nothing; the console
/// sends the same with Shift, Ctrl or Alt held.
fn sequence(key: Key, application_cursor: bool, out: &mut Vec<u8>) {
    if let Some(last) = cursor_final(key) {
        out.extend_from_slice(if application_cursor {
            b"\x1bO"
        } else {
            b"\x1b["
        });
        out.push(last);
    } else if let Some(text) = other_sequence(key) {
        out.extend_from_slice(text.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn press(key: Key, shift: bool, ctrl: bool, alt: bool) -> KeyPress {
        KeyPress {
            key,
            shift,
            ctrl,
            alt,
        }
    }

    fn bytes(press: KeyPress, application_cursor: bool) -> Vec<u8> {
        let mut out = Vec::new();
        encode(press, application_cursor, &mut out);
        out
    }

    fn plain(key: Key) -> Vec<u8> {
        bytes(press(key, false, false, false), false)
    }

    #[test]
    fn characters_and_their_controls() {
        assert_eq!(plain(Key::Char('a')), b"a");
        assert_eq!(plain(Key::Char('é')), "é".as_bytes());
        assert_eq!(bytes(press(Key::Char('c'), false, true, false), false), [3]);
        assert_eq!(bytes(press(Key::Char('D'), true, true, false), false), [4]);
        assert_eq!(
            bytes(press(Key::Char('['), false, true, false), false),
            [0x1b]
        );
        assert_eq!(bytes(press(Key::Char(' '), false, true, false), false), [0]);
        assert_eq!(
            bytes(press(Key::Char('?'), true, true, false), false),
            [0x7f]
        );
        assert_eq!(
            bytes(press(Key::Char('1'), false, true, false), false),
            b"1"
        );
        assert_eq!(
            bytes(press(Key::Char('x'), false, false, true), false),
            b"\x1bx"
        );
        assert_eq!(
            bytes(press(Key::Char('k'), false, true, true), false),
            [0x1b, 11]
        );
        let others: Vec<Option<u8>> = ['\\', ']', '^', '_', '/', '2', '8']
            .into_iter()
            .map(control)
            .collect();
        assert_eq!(
            others,
            [
                Some(0x1c),
                Some(0x1d),
                Some(0x1e),
                Some(0x1f),
                Some(0x1f),
                Some(0),
                Some(0x7f)
            ]
        );
    }

    #[test]
    fn keys_that_type_nothing() {
        assert_eq!(plain(Key::Enter), b"\r");
        assert_eq!(plain(Key::Escape), b"\x1b");
        assert_eq!(plain(Key::Backspace), b"\x7f");
        assert_eq!(bytes(press(Key::Backspace, false, true, false), false), [8]);
        assert_eq!(plain(Key::Tab), b"\t");
        assert_eq!(
            bytes(press(Key::BackTab, true, false, false), false),
            b"\x1b[Z"
        );
        assert_eq!(
            bytes(press(Key::Enter, false, false, true), false),
            b"\x1b\r"
        );
        assert_eq!(
            bytes(press(Key::Escape, false, false, true), false),
            b"\x1b\x1b"
        );
        assert_eq!(bytes(press(Key::Tab, false, false, true), false), b"\x1b\t");
    }

    #[test]
    fn cursor_keys_follow_the_cursor_key_mode() {
        assert_eq!(plain(Key::Up), b"\x1b[A");
        assert_eq!(bytes(press(Key::Up, false, false, false), true), b"\x1bOA");
        assert_eq!(bytes(press(Key::Left, false, true, false), true), b"\x1bOD");
        assert_eq!(
            bytes(press(Key::Right, true, false, true), false),
            b"\x1b[C"
        );
        assert_eq!(plain(Key::Down), b"\x1b[B");
    }

    #[test]
    fn function_and_editing_keys_are_the_console_s() {
        assert_eq!(plain(Key::F(1)), b"\x1b[[A");
        assert_eq!(plain(Key::F(5)), b"\x1b[[E");
        assert_eq!(
            bytes(press(Key::F(2), true, false, false), false),
            b"\x1b[[B"
        );
        assert_eq!(plain(Key::F(6)), b"\x1b[17~");
        assert_eq!(plain(Key::F(10)), b"\x1b[21~");
        assert_eq!(plain(Key::F(11)), b"\x1b[23~");
        assert_eq!(plain(Key::F(12)), b"\x1b[24~");
        assert_eq!(plain(Key::Home), b"\x1b[1~");
        assert_eq!(plain(Key::Insert), b"\x1b[2~");
        assert_eq!(plain(Key::Delete), b"\x1b[3~");
        assert_eq!(plain(Key::End), b"\x1b[4~");
        assert_eq!(plain(Key::PageUp), b"\x1b[5~");
        assert_eq!(
            bytes(press(Key::PageDown, false, true, false), false),
            b"\x1b[6~"
        );
        assert_eq!(plain(Key::F(13)), b"");
    }
}
