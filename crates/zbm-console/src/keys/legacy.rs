//! The sequences of the keys that type nothing: the Linux console's, which
//! are the same with Shift, Ctrl or Alt held, and xterm's, which carry the
//! modifiers held as a parameter (`ESC [ 1 ; 5 D` for Ctrl-Left).

use std::io::Write as _;

use zbm_display::keys::{Key, KeyPress};

use super::{Flavour, Modes};

/// The parameter of the modifiers held, as xterm sends it: 1, plus 1 for
/// Shift, 2 for Alt, 4 for Ctrl and 8 for the logo key (xterm's Meta,
/// kitty's Super).
pub fn modifier_parameter(press: KeyPress) -> u8 {
    1 + u8::from(press.shift)
        + 2 * u8::from(press.alt)
        + 4 * u8::from(press.ctrl)
        + 8 * u8::from(press.logo)
}

/// F13 to F24 on xterm: F1 to F12 with Shift, as its terminal description
/// has them (`kf13=\E[1;2P`).
fn xterm_function(press: KeyPress) -> KeyPress {
    match press.key {
        Key::F(number @ 13..=24) => KeyPress {
            key: Key::F(number - 12),
            shift: true,
            ..press
        },
        _ => press,
    }
}

/// The final character of the keys sent as `ESC [ final` or `ESC O final`:
/// the cursor keys, Home and End on xterm, F1 to F4 on xterm.
fn final_of(key: Key, flavour: Flavour) -> Option<u8> {
    let xterm = flavour == Flavour::Xterm;
    Some(match key {
        Key::Up => b'A',
        Key::Down => b'B',
        Key::Right => b'C',
        Key::Left => b'D',
        Key::Home if xterm => b'H',
        Key::End if xterm => b'F',
        Key::F(number @ 1..=4) if xterm => b'P' + number - 1,
        _ => return None,
    })
}

/// The number of the keys sent as `ESC [ number ~`.
fn number_of(key: Key, flavour: Flavour) -> Option<u8> {
    let xterm = flavour == Flavour::Xterm;
    Some(match key {
        Key::Home => 1,
        Key::Insert => 2,
        Key::Delete => 3,
        Key::End => 4,
        Key::PageUp => 5,
        Key::PageDown => 6,
        Key::F(5) if xterm => 15,
        Key::F(number @ 6..=10) => number + 11,
        // F13 and F14 follow on the Linux console (`kf13=\E[25~` in its
        // terminal description), then F15 to F20; it has no F21 to F24.
        Key::F(number @ 11..=14) => number + 12,
        Key::F(number @ 15..=16) => number + 13,
        Key::F(number @ 17..=20) => number + 14,
        _ => return None,
    })
}

/// Appends the sequence of `press`, a key that types nothing, to `out`.
pub fn sequence(press: KeyPress, modes: Modes, out: &mut Vec<u8>) {
    let flavour = modes.flavour;
    let press = match flavour {
        Flavour::Xterm => xterm_function(press),
        Flavour::Linux => press,
    };
    let parameter = match flavour {
        Flavour::Xterm => modifier_parameter(press),
        Flavour::Linux => 1,
    };
    // The Linux console sends `ESC [ [ letter` for F1 to F5.
    if flavour == Flavour::Linux
        && let Key::F(number @ 1..=5) = press.key
    {
        out.extend_from_slice(&[0x1b, b'[', b'[', b'A' + number - 1]);
    } else if let Some(last) = final_of(press.key, flavour) {
        let is_function = matches!(press.key, Key::F(_));
        if parameter > 1 {
            let _ = write!(out, "\x1b[1;{parameter}");
            out.push(last);
        } else if is_function || modes.application_cursor {
            out.extend_from_slice(&[0x1b, b'O', last]);
        } else {
            out.extend_from_slice(&[0x1b, b'[', last]);
        }
    } else if let Some(number) = number_of(press.key, flavour) {
        if parameter > 1 {
            let _ = write!(out, "\x1b[{number};{parameter}~");
        } else {
            let _ = write!(out, "\x1b[{number}~");
        }
    }
}
