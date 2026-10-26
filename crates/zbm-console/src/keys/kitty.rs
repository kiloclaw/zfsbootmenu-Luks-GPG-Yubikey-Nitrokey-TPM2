//! The kitty keyboard protocol
//! (<https://sw.kovidgoyal.net/kitty/keyboard-protocol/>), which programs
//! turn on to tell keys apart that the old sequences mix up: Escape from
//! the start of a sequence, Ctrl-I from Tab. Keys are sent as
//! `ESC [ code ; modifiers u`, where `code` is the key's Unicode code point
//! unshifted, and the modifiers are xterm's parameter.
//!
//! The console sends only presses (it reads no releases), and no alternate
//! keys: the key events it reads give the character typed, not the key's
//! unshifted character on every layout. The keys that have sequences of
//! their own (the cursor keys, the function keys) keep them, with their
//! modifiers, as the protocol allows.

use std::io::Write as _;

use zbm_display::keys::{Key, KeyPress};

use super::legacy::modifier_parameter;

/// Keys that would be ambiguous are sent as `ESC [ code u`: Escape, and
/// characters with Ctrl or Alt.
pub const DISAMBIGUATE: u8 = 1;

/// Every key is sent as `ESC [ code u`, characters too.
pub const ALL_KEYS: u8 = 8;

/// With [`ALL_KEYS`], the text a key types follows its modifiers.
pub const TEXT: u8 = 16;

/// The code of a character key: the character, lower case for a letter
/// typed with Shift.
fn code_of(c: char) -> u32 {
    u32::from(c.to_ascii_lowercase())
}

/// Appends `ESC [ code ; modifiers ; text u` to `out`, leaving out the
/// fields that say nothing.
fn write_key(out: &mut Vec<u8>, code: u32, parameter: u8, text: Option<char>) {
    let _ = write!(out, "\x1b[{code}");
    if parameter > 1 || text.is_some() {
        let _ = write!(out, ";{parameter}");
    }
    if let Some(text) = text {
        let _ = write!(out, ";{}", u32::from(text));
    }
    out.push(b'u');
}

/// Appends the kitty sequence of `press` with the protocol's `flags` to
/// `out`; `None` when the key keeps the sequence it has without the
/// protocol.
pub fn encode(press: KeyPress, flags: u8, out: &mut Vec<u8>) -> Option<()> {
    let all = flags & ALL_KEYS != 0;
    let parameter = modifier_parameter(press);
    let held = press.ctrl || press.alt;
    let code = match press.key {
        Key::Char(c) => {
            if !all && !held {
                return None;
            }
            let text = (all && flags & TEXT != 0 && !held && !c.is_control()).then_some(c);
            write_key(out, code_of(c), parameter, text);
            return Some(());
        }
        Key::Escape => 27,
        Key::Enter if all || parameter > 1 => 13,
        Key::Tab if all || parameter > 1 => 9,
        Key::Backspace if all || parameter > 1 => 127,
        Key::BackTab if all => {
            write_key(
                out,
                9,
                modifier_parameter(KeyPress {
                    shift: true,
                    ..press
                }),
                None,
            );
            return Some(());
        }
        _ => return None,
    };
    write_key(out, code, parameter, None);
    Some(())
}
