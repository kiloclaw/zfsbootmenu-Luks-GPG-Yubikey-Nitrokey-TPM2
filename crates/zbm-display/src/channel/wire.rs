//! The channel's encoding: a tag byte, then the fields, numbers in
//! little-endian, text as its length and its UTF-8. Decoding takes what a
//! peer sent, which may be anything: it refuses what is not exactly one
//! message, rather than guess.

use super::{Caps, FromClient, FromConsole, Run};
use crate::decor::{Decoration, Underline};
use crate::faces::FontFace;
use crate::frame::Rgb;
use crate::grid::CellStyle;
use crate::keys::{Key, KeyPress, Media};

/// The most cells a run may have: a row of the widest screen.
pub const MAX_CELLS: usize = 4096;

/// The longest text a cell may have, in bytes.
const MAX_CELL: usize = 64;

/// The bytes of a message, written field by field.
#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn rgb(&mut self, Rgb(red, green, blue): Rgb) {
        self.0.extend_from_slice(&[red, green, blue]);
    }

    /// Text of at most `u16::MAX` bytes; longer text is cut at a character.
    fn text(&mut self, text: &str) {
        let text = cut(text, usize::from(u16::MAX));
        self.u16(u16::try_from(text.len()).unwrap_or(u16::MAX));
        self.0.extend_from_slice(text.as_bytes());
    }

    /// A cell's text, of at most [`MAX_CELL`] bytes.
    fn cell(&mut self, text: &str) {
        let text = cut(text, MAX_CELL);
        self.u8(u8::try_from(text.len()).unwrap_or(u8::MAX));
        self.0.extend_from_slice(text.as_bytes());
    }

    fn style(&mut self, style: &CellStyle) {
        self.rgb(style.fg);
        self.rgb(style.bg);
        let decoration = style.decoration;
        self.u8(underline_number(decoration.underline));
        match decoration.underline_colour {
            Some(colour) => {
                self.bool(true);
                self.rgb(colour);
            }
            None => self.bool(false),
        }
        self.bool(decoration.strikeout);
        self.u8(face_number(style.face));
    }

    fn run(&mut self, run: &Run) {
        self.u16(run.row);
        self.u16(run.column);
        self.style(&run.style);
        let count = run.cells.len().min(MAX_CELLS);
        self.u16(u16::try_from(count).unwrap_or(u16::MAX));
        for cell in run.cells.iter().take(count) {
            self.cell(cell);
        }
    }

    fn press(&mut self, press: &KeyPress) {
        let (tag, value) = key_number(press.key);
        self.u8(tag);
        self.u32(value);
        let modifiers = [press.shift, press.ctrl, press.alt, press.logo, press.keypad];
        let bits = modifiers
            .iter()
            .enumerate()
            .filter(|(_, held)| **held)
            .fold(0_u8, |bits, (index, _)| bits | 1 << index);
        self.u8(bits);
    }
}

/// `text`, cut at a character to at most `bytes` bytes.
fn cut(text: &str, bytes: usize) -> &str {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// The fields of a message, read in order; `None` past its end.
struct Reader<'b> {
    bytes: &'b [u8],
}

impl<'b> Reader<'b> {
    fn take(&mut self, count: usize) -> Option<&'b [u8]> {
        let (taken, rest) = self.bytes.split_at_checked(count)?;
        self.bytes = rest;
        Some(taken)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    fn rgb(&mut self) -> Option<Rgb> {
        let [red, green, blue] = self.take(3)?.try_into().ok()?;
        Some(Rgb(red, green, blue))
    }

    fn text(&mut self) -> Option<String> {
        let length = usize::from(self.u16()?);
        Some(std::str::from_utf8(self.take(length)?).ok()?.to_owned())
    }

    fn cell(&mut self) -> Option<String> {
        let length = usize::from(self.u8()?);
        if length > MAX_CELL {
            return None;
        }
        Some(std::str::from_utf8(self.take(length)?).ok()?.to_owned())
    }

    fn style(&mut self) -> Option<CellStyle> {
        let fg = self.rgb()?;
        let bg = self.rgb()?;
        let underline = underline_of(self.u8()?)?;
        let underline_colour = if self.bool()? {
            Some(self.rgb()?)
        } else {
            None
        };
        let strikeout = self.bool()?;
        let face = face_of(self.u8()?)?;
        Some(CellStyle {
            fg,
            bg,
            decoration: Decoration {
                underline,
                underline_colour,
                strikeout,
            },
            face,
        })
    }

    fn run(&mut self) -> Option<Run> {
        let row = self.u16()?;
        let column = self.u16()?;
        let style = self.style()?;
        let count = usize::from(self.u16()?);
        if count > MAX_CELLS {
            return None;
        }
        let cells = (0..count)
            .map(|_| self.cell())
            .collect::<Option<Vec<_>>>()?;
        Some(Run {
            row,
            column,
            style,
            cells,
        })
    }

    fn press(&mut self) -> Option<KeyPress> {
        let key = key_of(self.u8()?, self.u32()?)?;
        let bits = self.u8()?;
        if bits >> 5 != 0 {
            return None;
        }
        let held = |index: u8| bits & 1 << index != 0;
        Some(KeyPress {
            key,
            shift: held(0),
            ctrl: held(1),
            alt: held(2),
            logo: held(3),
            keypad: held(4),
        })
    }

    /// The value read, when nothing follows it.
    fn end<T>(self, value: T) -> Option<T> {
        self.bytes.is_empty().then_some(value)
    }
}

fn underline_number(underline: Underline) -> u8 {
    match underline {
        Underline::None => 0,
        Underline::Single => 1,
        Underline::Double => 2,
        Underline::Curly => 3,
        Underline::Dotted => 4,
        Underline::Dashed => 5,
    }
}

fn underline_of(number: u8) -> Option<Underline> {
    Some(match number {
        0 => Underline::None,
        1 => Underline::Single,
        2 => Underline::Double,
        3 => Underline::Curly,
        4 => Underline::Dotted,
        5 => Underline::Dashed,
        _ => return None,
    })
}

fn face_number(face: FontFace) -> u8 {
    match face {
        FontFace::Regular => 0,
        FontFace::Bold => 1,
        FontFace::Italic => 2,
        FontFace::BoldItalic => 3,
    }
}

fn face_of(number: u8) -> Option<FontFace> {
    Some(match number {
        0 => FontFace::Regular,
        1 => FontFace::Bold,
        2 => FontFace::Italic,
        3 => FontFace::BoldItalic,
        _ => return None,
    })
}

/// The keys without a value, in the order of their numbers from 1.
const KEYS: [Key; 18] = [
    Key::Enter,
    Key::Escape,
    Key::Backspace,
    Key::Tab,
    Key::BackTab,
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
    Key::Insert,
    Key::Delete,
    Key::Print,
    Key::Pause,
    Key::Menu,
];

/// The media keys, in the order of their numbers.
const MEDIA: [Media; 7] = [
    Media::Mute,
    Media::VolumeDown,
    Media::VolumeUp,
    Media::PlayPause,
    Media::Stop,
    Media::Next,
    Media::Previous,
];

/// A key's number, and its value: 0 for a character, 100 for a function
/// key, 101 for a media key, else its place in [`KEYS`] from 1.
fn key_number(key: Key) -> (u8, u32) {
    match key {
        Key::Char(c) => (0, u32::from(c)),
        Key::F(number) => (100, u32::from(number)),
        Key::Media(media) => {
            let place = MEDIA.iter().position(|&known| known == media).unwrap_or(0);
            (101, u32::try_from(place).unwrap_or(0))
        }
        other => {
            let place = KEYS.iter().position(|&known| known == other).unwrap_or(0);
            (u8::try_from(place + 1).unwrap_or(u8::MAX), 0)
        }
    }
}

fn key_of(tag: u8, value: u32) -> Option<Key> {
    match tag {
        0 => Some(Key::Char(char::from_u32(value)?)),
        100 => Some(Key::F(
            u8::try_from(value)
                .ok()
                .filter(|&n| (1..=24).contains(&n))?,
        )),
        101 => Some(Key::Media(*MEDIA.get(usize::try_from(value).ok()?)?)),
        _ if value == 0 => KEYS.get(usize::from(tag).checked_sub(1)?).copied(),
        _ => None,
    }
}

/// The bytes a run takes in a message.
pub fn run_length(run: &Run) -> usize {
    let mut writer = Writer::default();
    writer.run(run);
    writer.0.len()
}

impl FromClient {
    /// The message as one packet.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        match self {
            FromClient::Hello { version } => {
                writer.u8(1);
                writer.u16(*version);
            }
            FromClient::Cells(runs) => {
                writer.u8(2);
                writer.u16(u16::try_from(runs.len()).unwrap_or(u16::MAX));
                for run in runs.iter().take(usize::from(u16::MAX)) {
                    writer.run(run);
                }
            }
            FromClient::Cursor(position) => {
                writer.u8(3);
                writer.bool(position.is_some());
                let (column, row) = position.unwrap_or_default();
                writer.u16(column);
                writer.u16(row);
            }
            FromClient::Secret(secret) => {
                writer.u8(4);
                writer.bool(*secret);
            }
            FromClient::SwitchLayout(name) => {
                writer.u8(5);
                writer.text(name);
            }
            FromClient::Announce(text) => {
                writer.u8(6);
                writer.text(text);
            }
            FromClient::Lend => writer.u8(7),
            FromClient::Take => writer.u8(8),
        }
        writer.0
    }

    /// The message a peer sent as `bytes`; `None` when it is none.
    pub fn decode(bytes: &[u8]) -> Option<FromClient> {
        let mut reader = Reader { bytes };
        let message = match reader.u8()? {
            1 => FromClient::Hello {
                version: reader.u16()?,
            },
            2 => {
                let count = reader.u16()?;
                let runs = (0..count)
                    .map(|_| reader.run())
                    .collect::<Option<Vec<_>>>()?;
                FromClient::Cells(runs)
            }
            3 => {
                let shown = reader.bool()?;
                let position = (reader.u16()?, reader.u16()?);
                // A hidden cursor has no place: one encoding per message.
                if !shown && position != (0, 0) {
                    return None;
                }
                FromClient::Cursor(shown.then_some(position))
            }
            4 => FromClient::Secret(reader.bool()?),
            5 => FromClient::SwitchLayout(reader.text()?),
            6 => FromClient::Announce(reader.text()?),
            7 => FromClient::Lend,
            8 => FromClient::Take,
            _ => return None,
        };
        reader.end(message)
    }
}

impl FromConsole {
    /// The message as one packet.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        match self {
            FromConsole::Hello { version } => {
                writer.u8(1);
                writer.u16(*version);
            }
            FromConsole::Caps(caps) => {
                writer.u8(2);
                writer.u16(caps.columns);
                writer.u16(caps.rows);
                writer.bool(caps.true_colour);
            }
            FromConsole::Key { press, layout } => {
                writer.u8(3);
                writer.press(press);
                writer.text(layout);
            }
            FromConsole::Resize { columns, rows } => {
                writer.u8(4);
                writer.u16(*columns);
                writer.u16(*rows);
            }
            FromConsole::Bell => writer.u8(5),
        }
        writer.0
    }

    /// The message a peer sent as `bytes`; `None` when it is none.
    pub fn decode(bytes: &[u8]) -> Option<FromConsole> {
        let mut reader = Reader { bytes };
        let message = match reader.u8()? {
            1 => FromConsole::Hello {
                version: reader.u16()?,
            },
            2 => FromConsole::Caps(Caps {
                columns: reader.u16()?,
                rows: reader.u16()?,
                true_colour: reader.bool()?,
            }),
            3 => FromConsole::Key {
                press: reader.press()?,
                layout: reader.text()?,
            },
            4 => FromConsole::Resize {
                columns: reader.u16()?,
                rows: reader.u16()?,
            },
            5 => FromConsole::Bell,
            _ => return None,
        };
        reader.end(message)
    }
}
