//! The key presses of the terminal, read from its bytes into buffers that
//! are wiped once decoded: what is typed may be a passphrase.
//!
//! The start-up view reads keys with this rather than with the terminal
//! library's event reader, which keeps the last bytes it read in a buffer
//! of its own until more come: a passphrase typed at start-up would stay in
//! the memory of the start-up program, which runs until the boot
//! (`crates/DESIGN.md`, "Secrets"). Keys decode into the presses the
//! display's keyboards give ([`KeyPress`]), as that reader would decode
//! them: Shift with capitals, Ctrl with the control characters, Alt with
//! Escape before a key, and the sequences of the Linux console and of xterm
//! for the keys that type nothing.

use std::fs::File;
use std::io::{self, Read as _};
use std::os::fd::AsFd as _;
use std::time::{Duration, Instant};

use ratatui::crossterm::terminal;
use zbm_display::keys::{Key, KeyPress};

use super::keymap::{Modifiers, press};
use super::startup::Keys;

/// `ESC`.
const ESCAPE: u8 = 0x1b;

/// How much is read at once.
const BUFFER: usize = 256;

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

/// What a decoded key is overwritten with.
fn blank() -> KeyPress {
    KeyPress::BLANK
}

/// The keys of the terminal on standard input, which is in raw mode while
/// the start-up view has it.
pub struct TerminalKeys {
    /// Standard input, read without a buffer of the standard library.
    input: File,
    /// Whether this set raw mode, which it ends.
    raw: bool,
    /// What was read and not decoded yet: the start of a key's sequence.
    pending: Vec<u8>,
    /// The keys decoded, and the next one to give.
    keys: Vec<KeyPress>,
    next: usize,
    /// How long the rest of a sequence may take
    /// ([`CONSOLE_ESCAPE_WAIT`], [`SERIAL_ESCAPE_WAIT`]).
    escape_wait: Duration,
    /// When the last bytes came.
    last_read: Option<Instant>,
    /// Whether the last byte decoded was a CR, whose LF may come in the
    /// next read: then it is part of the same Enter.
    after_cr: bool,
}

impl TerminalKeys {
    /// The keys of the terminal, which goes into raw mode; the rest of a
    /// key's sequence may take `escape_wait` to come.
    ///
    /// # Errors
    ///
    /// When standard input cannot be used, or raw mode set.
    pub fn new(escape_wait: Duration) -> io::Result<TerminalKeys> {
        let input = File::from(io::stdin().as_fd().try_clone_to_owned()?);
        let mut keys = TerminalKeys::on(input, escape_wait);
        keys.take()?;
        Ok(keys)
    }

    /// The keys read from `input`, whose mode this leaves alone.
    pub fn on(input: File, escape_wait: Duration) -> TerminalKeys {
        TerminalKeys {
            input,
            raw: false,
            pending: Vec::with_capacity(2 * BUFFER),
            // Each key takes a byte at least, so the keys of what is
            // pending fit without the list growing, which would leave a
            // copy of them behind unwiped.
            keys: Vec::with_capacity(2 * BUFFER),
            next: 0,
            escape_wait,
            last_read: None,
            after_cr: false,
        }
    }

    /// Reads and decodes what comes within `timeout`. The start of a
    /// sequence is decoded alone, as the Escape key, only once nothing more
    /// came for the escape wait, however short `timeout` is.
    ///
    /// # Errors
    ///
    /// When the input cannot be read, or ended.
    #[allow(
        clippy::disallowed_methods,
        reason = "the key reader is the terminal's adapter; gaps between bytes are wall-clock time"
    )]
    fn fill(&mut self, timeout: Duration) -> io::Result<()> {
        let since = self.last_read.map(|at| at.elapsed());
        let wait = match since {
            Some(since) if !self.pending.is_empty() => {
                self.escape_wait.saturating_sub(since).min(timeout)
            }
            _ => timeout,
        };
        if self.read(wait)? {
            self.last_read = Some(Instant::now());
        }
        let waited = self
            .last_read
            .is_none_or(|at| at.elapsed() >= self.escape_wait);
        self.decode(waited);
        Ok(())
    }

    /// Reads what came within `timeout`: whether anything did.
    ///
    /// # Errors
    ///
    /// When the input cannot be read, or ended.
    fn read(&mut self, timeout: Duration) -> io::Result<bool> {
        if !zbm_sys::wait_readable(&self.input, Some(timeout))? {
            return Ok(false);
        }
        let mut buffer = [0_u8; BUFFER];
        let read = self.input.read(&mut buffer);
        if let Ok(count) = read {
            self.pending
                .extend_from_slice(buffer.get(..count).unwrap_or_default());
        }
        zbm_sys::wipe(&mut buffer);
        match read? {
            0 => Err(io::ErrorKind::UnexpectedEof.into()),
            _ => Ok(true),
        }
    }

    /// Decodes what was read, the start of a sequence too when `last`; the
    /// bytes decoded are wiped.
    fn decode(&mut self, last: bool) {
        // The carriage return counts for the byte right after it alone.
        if self.after_cr && !self.pending.is_empty() {
            self.after_cr = false;
            if self.pending.first() == Some(&b'\n') {
                self.pending.copy_within(1.., 0);
                let end = self.pending.len() - 1;
                zbm_sys::wipe(self.pending.get_mut(end..).unwrap_or_default());
                self.pending.truncate(end);
            }
        }
        let decoded = self.keys.len();
        let used = decode(&self.pending, last, &mut self.keys);
        if used > 0 {
            let bytes = self.pending.get(..used).unwrap_or_default();
            let keys = self.keys.get(decoded..).unwrap_or_default();
            self.after_cr = ends_with_carriage_return(bytes, keys);
        }
        let rest = self.pending.len() - used;
        self.pending.copy_within(used.., 0);
        zbm_sys::wipe(self.pending.get_mut(rest..).unwrap_or_default());
        self.pending.truncate(rest);
    }

    /// Wipes the keys decoded and given.
    fn forget(&mut self) {
        zbm_sys::wipe_values(&mut self.keys, blank());
        self.next = 0;
    }

    /// Wipes everything read.
    fn forget_all(&mut self) {
        self.forget();
        zbm_sys::wipe_vec(&mut self.pending);
        self.last_read = None;
        self.after_cr = false;
    }
}

impl Keys for TerminalKeys {
    /// The next key press, waiting at most `timeout` for it.
    fn key(&mut self, timeout: Duration) -> io::Result<Option<KeyPress>> {
        if self.next >= self.keys.len() {
            self.forget();
            self.fill(timeout)?;
        }
        let key = self.keys.get(self.next).copied();
        if key.is_some() {
            self.next += 1;
        }
        Ok(key)
    }

    /// Ends raw mode; what was typed and not used is dropped.
    fn release(&mut self) -> io::Result<()> {
        self.forget_all();
        if self.raw {
            self.raw = false;
            terminal::disable_raw_mode()?;
        }
        Ok(())
    }

    fn take(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()?;
        self.raw = true;
        Ok(())
    }

    /// The terminal's size, as the kernel knows it: what `stty` set on a
    /// serial console, the font's grid on a virtual console.
    fn size(&mut self) -> Option<(u16, u16)> {
        let size = terminal::window_size().ok()?;
        Some((size.columns, size.rows))
    }
}

impl Drop for TerminalKeys {
    fn drop(&mut self) {
        self.forget_all();
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}

#[cfg(test)]
mod tests;
