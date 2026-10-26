//! The bytes a key press sends to the program in the terminal.
//!
//! The programs run with one of two terminal descriptions ([`Flavour`]):
//! `xterm-256color` when the image has it, whose sequences Alacritty's
//! terminal is built for, or `linux`, as on the kernel console. Characters
//! are the same in both: control characters with Ctrl, Escape before them
//! with Alt. The keys that type nothing send the sequences of the terminal
//! named, unless a program asked for the kitty keyboard protocol. The
//! terminal's modes change some of them: the cursor keys (DECCKM), the
//! keypad (DECKPAM) and Enter (LNM).
//!
//! What is typed may be a passphrase: the bytes are written into a buffer
//! the caller wipes once they are sent (`zbm_sys::wipe_vec`), and numbers
//! are written straight into it, never through a string of their own.

use zbm_display::keys::{Key, KeyPress};

mod kitty;
mod legacy;

pub use kitty::{ALL_KEYS, DISAMBIGUATE, TEXT};

/// `ESC`.
const ESCAPE: u8 = 0x1b;

/// The terminal description the programs read keys through.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Flavour {
    /// `TERM=linux`, the kernel console's keys.
    #[default]
    Linux,
    /// `TERM=xterm-256color`, xterm's keys, with modifiers in their
    /// sequences.
    Xterm,
}

/// Where images keep terminal descriptions: where the build host keeps
/// them, as dracut and mkinitcpio copy them.
pub const TERMINFO: [&str; 3] = ["/etc/terminfo", "/lib/terminfo", "/usr/share/terminfo"];

impl Flavour {
    /// The terminal description of an image in which `has` says which
    /// files exist: xterm's when it has `xterm-256color`.
    pub fn of_image(has: impl Fn(&std::path::Path) -> bool) -> Flavour {
        let found = TERMINFO
            .iter()
            .any(|dir| has(&std::path::Path::new(dir).join("x/xterm-256color")));
        if found {
            Flavour::Xterm
        } else {
            Flavour::Linux
        }
    }

    /// The value of `TERM` for the programs.
    pub fn term(self) -> &'static str {
        match self {
            Flavour::Linux => "linux",
            Flavour::Xterm => "xterm-256color",
        }
    }
}

/// What the terminal's state asks of the keys.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modes {
    /// The terminal description.
    pub flavour: Flavour,
    /// The cursor keys send `ESC O` sequences (DECCKM).
    pub application_cursor: bool,
    /// The keypad sends `ESC O` sequences (DECKPAM).
    pub application_keypad: bool,
    /// Enter sends a carriage return and a line feed (LNM).
    pub new_line: bool,
    /// The flags of the kitty keyboard protocol a program asked for:
    /// [`DISAMBIGUATE`], [`ALL_KEYS`] and [`TEXT`]; the others are not
    /// sent.
    pub kitty: u8,
}

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

/// The final character of a keypad key in application keypad mode, which
/// differs between the two terminals for the operators.
fn keypad_final(key: Key, flavour: Flavour) -> Option<u8> {
    let xterm = flavour == Flavour::Xterm;
    Some(match key {
        Key::Char(digit @ '0'..='9') => b'p' + u8::try_from(digit).ok()? - b'0',
        Key::Char('.' | ',') => b'n',
        Key::Char('+') if xterm => b'k',
        Key::Char('+') => b'l',
        Key::Char('-') if xterm => b'm',
        Key::Char('-') => b'S',
        Key::Char('*') if xterm => b'j',
        Key::Char('*') => b'R',
        Key::Char('/') if xterm => b'o',
        Key::Char('/') => b'Q',
        Key::Enter => b'M',
        _ => return None,
    })
}

/// Appends the bytes of `press` to `out`, for a terminal in `modes`.
pub fn encode(press: KeyPress, modes: Modes, out: &mut Vec<u8>) {
    if modes.kitty != 0 && kitty::encode(press, modes.kitty, out).is_some() {
        return;
    }
    if press.keypad
        && modes.application_keypad
        && let Some(last) = keypad_final(press.key, modes.flavour)
    {
        out.extend_from_slice(&[ESCAPE, b'O', last]);
        return;
    }
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
            if modes.new_line {
                out.push(b'\n');
            }
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
        _ => legacy::sequence(press, modes, out),
    }
}

#[cfg(test)]
mod tests;
