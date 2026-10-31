//! The serial console too (ROADMAP.md UI9, C81): the screen the display
//! shows, written to a serial terminal as text with the 16 colours of
//! ECMA-48, and the keys typed there read back.
//!
//! [`SerialView`] writes only the rows that changed since the terminal got
//! the last screen, without blocking: a slow line, or one nobody listens
//! on, takes what it can, and of the screens that come meanwhile only the
//! latest is written, so the terminal always ends with the screen shown.
//! The terminal's size is the one given (`zbm.columns`, `zbm.lines`): a
//! screen larger than it is cut there, and the console holds the screen
//! to it while it is typed on. [`SerialInput`] decodes what is typed into
//! key presses, waiting [`SERIAL_ESCAPE_WAIT`] for the rest of a sequence
//! that came in pieces. Every copy of a screen and of what is typed is
//! wiped when it goes: a passphrase shown by mistake does not stay in
//! memory.

use std::io::{self, Write};
use std::time::Instant;

use zbm_display::faces::FontFace;
use zbm_display::frame::Rgb;
use zbm_display::grid::CellStyle;
use zbm_display::keys::{Key, KeyPress};
use zbm_display::term_keys::{SERIAL_ESCAPE_WAIT, decode, ends_with_carriage_return};

use crate::scheme::Scheme;
use crate::screen::styled::{Styled, StyledCell};

/// The default size of a serial terminal, as the legacy runtime sets it.
pub const DEFAULT_SIZE: (u16, u16) = (80, 25);

/// A screen as the terminal gets it: each row's bytes, and the cursor.
#[derive(Debug, Default, Eq, PartialEq)]
struct Rendered {
    rows: Vec<Vec<u8>>,
    cursor: Option<(u16, u16)>,
}

impl Drop for Rendered {
    fn drop(&mut self) {
        for row in &mut self.rows {
            zbm_sys::wipe_vec(row);
        }
    }
}

/// The colour of the 16 of `scheme` nearest to `rgb`.
fn nearest(rgb: Rgb, scheme: &Scheme) -> u8 {
    let distance = |other: Rgb| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(rgb.0, other.0) + d(rgb.1, other.1) + d(rgb.2, other.2)
    };
    (0..16_u8)
        .min_by_key(|&index| distance(scheme.indexed(index)))
        .unwrap_or(7)
}

/// The SGR sequence of `style`: the terminal's default colours for the
/// scheme's default text and background, else the nearest of the 16.
fn sgr(style: &CellStyle, scheme: &Scheme) -> Vec<u8> {
    let mut codes = vec!["0".to_owned()];
    if matches!(style.face, FontFace::Bold | FontFace::BoldItalic) {
        codes.push("1".to_owned());
    }
    if style.decoration != zbm_display::decor::Decoration::default() {
        codes.push("4".to_owned());
    }
    let colour = |rgb: Rgb, default: Rgb, base: u8, bright: u8, plain: &str| {
        if rgb == default {
            return plain.to_owned();
        }
        let index = nearest(rgb, scheme);
        if index < 8 {
            (base + index).to_string()
        } else {
            (bright + index - 8).to_string()
        }
    };
    codes.push(colour(style.fg, scheme.foreground, 30, 90, "39"));
    codes.push(colour(style.bg, scheme.background, 40, 100, "49"));
    format!("\x1b[{}m", codes.join(";")).into_bytes()
}

/// Whether `cell` shows nothing a cleared cell would not.
fn is_blank(cell: &StyledCell, scheme: &Scheme) -> bool {
    cell.text == " "
        && cell.style.bg == scheme.background
        && cell.style.decoration == zbm_display::decor::Decoration::default()
}

/// The bytes that draw `cells` from the first column of a row `columns`
/// wide: what is past the last cell that shows something is cleared. A
/// wide character the edge cuts in two is a space.
fn render_row(all: &[StyledCell], columns: u16, scheme: &Scheme) -> Vec<u8> {
    let cells = all.get(..usize::from(columns)).unwrap_or(all);
    let end = cells
        .iter()
        .rposition(|cell| !cell.text.is_empty() && !is_blank(cell, scheme))
        .map_or(0, |last| last + 1);
    let mut bytes = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    for (column, cell) in cells.iter().take(end).enumerate() {
        // The second cell of a wide character, which the first drew.
        if cell.text.is_empty() {
            continue;
        }
        let style = sgr(&cell.style, scheme);
        if current.as_ref() != Some(&style) {
            bytes.extend_from_slice(&style);
            current = Some(style);
        }
        let is_cut = column + 1 == cells.len()
            && all.get(column + 1).is_some_and(|next| next.text.is_empty());
        if is_cut {
            bytes.push(b' ');
            continue;
        }
        for c in cell.text.chars() {
            let c = if c.is_control() { '?' } else { c };
            let mut buffer = [0; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
        }
    }
    if end < usize::from(columns) {
        bytes.extend_from_slice(b"\x1b[0m\x1b[K");
    }
    bytes
}

/// The screen written to a serial terminal of a given size: only what
/// changed, without blocking.
pub struct SerialView<W> {
    out: W,
    size: (u16, u16),
    scheme: &'static Scheme,
    /// What the terminal shows, as written; `None` before the first screen
    /// or after a repaint was asked for, when it gets the whole screen.
    shown: Option<Rendered>,
    /// The latest screen, rendered, and whether the terminal has it.
    latest: Rendered,
    is_latest_written: bool,
    /// The bytes being written, how many are, and what the terminal shows
    /// once they are.
    pending: Vec<u8>,
    sent: usize,
    target: Option<Rendered>,
}

impl<W: Write> SerialView<W> {
    /// The screen written to `out`, a terminal of `size` (columns, rows),
    /// in the colours of `scheme`.
    pub fn new(out: W, size: (u16, u16), scheme: &'static Scheme) -> SerialView<W> {
        SerialView {
            out,
            size: (size.0.max(1), size.1.max(1)),
            scheme,
            shown: None,
            latest: Rendered::default(),
            is_latest_written: true,
            pending: Vec::new(),
            sent: 0,
            target: None,
        }
    }

    /// The terminal's size: columns and rows.
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// Takes the screen `styled`, and writes what the terminal takes now.
    pub fn show(&mut self, styled: &Styled) {
        let (columns, rows) = self.size;
        let rendered = Rendered {
            rows: styled
                .rows
                .iter()
                .take(usize::from(rows))
                .map(|cells| render_row(cells, columns, self.scheme))
                .collect(),
            cursor: styled
                .cursor
                .filter(|&(column, row)| column < columns && row < rows),
        };
        if rendered != self.latest {
            self.latest = rendered;
            self.is_latest_written = false;
            // Changes the terminal got none of yet are made again from the
            // screen it shows, for the latest screen.
            if self.sent == 0 && self.target.take().is_some() {
                zbm_sys::wipe_vec(&mut self.pending);
            }
        }
        self.flush();
    }

    /// Writes the whole screen again with the next one: the terminal was
    /// cleared, or joined late, as asked with Ctrl+L.
    pub fn repaint(&mut self) {
        self.shown = None;
        if let Some(target) = &mut self.target {
            // What is being written ends on a screen that is not trusted
            // either.
            target.rows.clear();
        }
        self.is_latest_written = false;
    }

    /// Whether something is left to write.
    pub fn is_pending(&self) -> bool {
        self.sent < self.pending.len() || !self.is_latest_written
    }

    /// Writes what the terminal takes now: the rest of the bytes being
    /// written, then the changes to the latest screen.
    pub fn flush(&mut self) {
        loop {
            if self.sent >= self.pending.len() {
                zbm_sys::wipe_vec(&mut self.pending);
                self.sent = 0;
                if let Some(target) = self.target.take() {
                    // A repaint asked meanwhile left no rows to trust.
                    self.shown =
                        (!target.rows.is_empty() || self.latest.rows.is_empty()).then_some(target);
                }
                if self.is_latest_written {
                    return;
                }
                self.pending = self.changes();
                self.target = Some(Rendered {
                    rows: self.latest.rows.clone(),
                    cursor: self.latest.cursor,
                });
                self.is_latest_written = true;
            }
            match self
                .out
                .write(self.pending.get(self.sent..).unwrap_or_default())
            {
                Ok(0) => return,
                Ok(written) => self.sent += written,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return,
            }
        }
    }

    /// The bytes that turn what the terminal shows into the latest screen:
    /// the cursor hidden, the rows that differ, then the cursor where it is.
    fn changes(&self) -> Vec<u8> {
        let mut bytes = b"\x1b[?25l".to_vec();
        let shown = self.shown.as_ref();
        if shown.is_none() {
            bytes.extend_from_slice(b"\x1b[0m\x1b[H\x1b[2J");
        }
        let blank = b"\x1b[0m\x1b[K".to_vec();
        let count = shown
            .map_or(0, |shown| shown.rows.len())
            .max(self.latest.rows.len());
        for row in 0..count {
            let now = self.latest.rows.get(row).unwrap_or(&blank);
            let before = shown.map(|shown| shown.rows.get(row).unwrap_or(&blank));
            let is_same = before.map_or(now == &blank, |before| before == now);
            if is_same {
                continue;
            }
            bytes.extend_from_slice(format!("\x1b[{};1H", row + 1).as_bytes());
            bytes.extend_from_slice(now);
        }
        match self.latest.cursor {
            Some((column, row)) => {
                bytes.extend_from_slice(
                    format!("\x1b[{};{}H\x1b[?25h", row + 1, column + 1).as_bytes(),
                );
            }
            None => bytes.extend_from_slice(b"\x1b[0m"),
        }
        bytes
    }
}

impl<W> Drop for SerialView<W> {
    fn drop(&mut self) {
        zbm_sys::wipe_vec(&mut self.pending);
    }
}

/// What is typed on the serial terminal, decoded into key presses: a key
/// whose sequence came in pieces waits [`SERIAL_ESCAPE_WAIT`] for the
/// rest, then `ESC` alone is the Escape key.
#[derive(Debug, Default)]
pub struct SerialInput {
    /// What was read and not decoded yet: the start of a key's sequence.
    pending: Vec<u8>,
    /// When it came.
    since: Option<Instant>,
    /// Whether the last byte decoded was a CR, whose LF may come next.
    after_cr: bool,
}

impl SerialInput {
    /// Takes `bytes`, read at `now`; the keys they finish.
    pub fn take(&mut self, bytes: &[u8], now: Instant) -> Vec<KeyPress> {
        let bytes = match bytes.split_first() {
            // The LF of a CR LF that came in two reads.
            Some((b'\n', rest)) if self.after_cr && self.pending.is_empty() => {
                self.after_cr = false;
                rest
            }
            _ => bytes,
        };
        self.pending.extend_from_slice(bytes);
        self.decode(false, now)
    }

    /// When the sequence that waits is taken as it is, if one waits.
    pub fn deadline(&self) -> Option<Instant> {
        self.since.map(|since| since + SERIAL_ESCAPE_WAIT)
    }

    /// The keys of the sequence that waited, once its time ran out at
    /// `now`.
    pub fn expire(&mut self, now: Instant) -> Vec<KeyPress> {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            self.decode(true, now)
        } else {
            Vec::new()
        }
    }

    /// Decodes what is pending, all of it when `last`; what was decoded is
    /// wiped.
    fn decode(&mut self, last: bool, now: Instant) -> Vec<KeyPress> {
        let mut keys = Vec::new();
        let used = decode(&self.pending, last, &mut keys);
        if used > 0 {
            let decoded = self.pending.get(..used).unwrap_or_default();
            self.after_cr = ends_with_carriage_return(decoded, &keys);
        }
        let mut rest = self.pending.split_off(used);
        std::mem::swap(&mut rest, &mut self.pending);
        zbm_sys::wipe_vec(&mut rest);
        self.since = if self.pending.is_empty() {
            None
        } else {
            self.since.or(Some(now))
        };
        keys
    }
}

impl Drop for SerialInput {
    fn drop(&mut self) {
        zbm_sys::wipe_vec(&mut self.pending);
    }
}

/// Whether `keys` ask the serial terminal's screen to be written whole
/// again: Ctrl+L, which the programs get too.
pub fn has_repaint_key(keys: &[KeyPress]) -> bool {
    keys.iter()
        .any(|press| press.ctrl && !press.alt && press.key == Key::Char('l'))
}

#[cfg(test)]
mod tests;
