//! The client channel between the console on the display and the menu
//! (ADR 0004, ROADMAP.md UI6): the menu sends the cells it draws, and gets
//! the keys pressed with the layout they were typed on, instead of a
//! terminal between them. The supervisor makes the socket pair and hands
//! one end to each.
//!
//! Each message is one packet of a `SOCK_SEQPACKET` socket, in a binary
//! encoding of its own ([`wire`]): a tag byte, then the fields, numbers in
//! little-endian. Both sides start with [`FromClient::Hello`] and
//! [`FromConsole::Hello`], and a side of another [`VERSION`] is not talked
//! to: the menu draws on its terminal then.
//!
//! The secret flag travels in the same stream as the keys: the keys that
//! come while it is set belong to a passphrase, and the console keeps no
//! copy of them, nor of the screen.

use crate::grid::CellStyle;
use crate::keys::KeyPress;

pub mod wire;

/// The version of the channel's messages.
pub const VERSION: u16 = 1;

/// Cells of one row with one style, from a column: each cell's text, a
/// grapheme, or empty for the second cell of a wide character.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Run {
    /// The row, from 0 at the top.
    pub row: u16,
    /// The first column.
    pub column: u16,
    /// How they are drawn.
    pub style: CellStyle,
    /// The cells' text, one per cell.
    pub cells: Vec<String>,
}

/// What the console can show.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Caps {
    /// The columns of cells.
    pub columns: u16,
    /// The rows.
    pub rows: u16,
    /// Whether colours beyond the 16 of the console's palette are shown as
    /// they are.
    pub true_colour: bool,
}

/// What the menu sends the console.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FromClient {
    /// The first message, with the client's version.
    Hello {
        /// [`VERSION`].
        version: u16,
    },
    /// Cells drawn since the last frame; a frame may take several.
    Cells(Vec<Run>),
    /// Where the cursor is, or `None` when it is hidden; the frame is
    /// whole and the console shows it.
    Cursor(Option<(u16, u16)>),
    /// Whether the keys that follow belong to a secret.
    Secret(bool),
    /// Type with the image's keyboard layout of this name.
    SwitchLayout(String),
    /// A line for a screen reader.
    Announce(String),
    /// The screen is lent to the programs' terminal, as for the shell.
    Lend,
    /// The screen is the menu's again; the next frame is whole.
    Take,
}

/// What the console sends the menu.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FromConsole {
    /// The first message, with the console's version.
    Hello {
        /// [`VERSION`].
        version: u16,
    },
    /// What the console shows; sent after the hello.
    Caps(Caps),
    /// A key pressed, and the keyboard layout it was typed on.
    Key {
        /// The key.
        press: KeyPress,
        /// The layout's name.
        layout: String,
    },
    /// The cells changed in number, as when the font's size changed; the
    /// next frame is drawn whole.
    Resize {
        /// The columns.
        columns: u16,
        /// The rows.
        rows: u16,
    },
    /// The console's bell rang for the menu.
    Bell,
}

/// The runs of `cells` split into messages that each fit in one packet of
/// `limit` bytes: a run too long for one is split by columns.
pub fn cells_messages(runs: Vec<Run>, limit: usize) -> Vec<FromClient> {
    let room = limit.saturating_sub(CELLS_HEADER);
    let mut messages = Vec::new();
    let mut current: Vec<Run> = Vec::new();
    let mut size = 0;
    for run in runs.into_iter().flat_map(|run| split(run, room)) {
        let length = wire::run_length(&run);
        if !current.is_empty() && size + length > room {
            messages.push(FromClient::Cells(std::mem::take(&mut current)));
            size = 0;
        }
        size += length;
        current.push(run);
    }
    if !current.is_empty() {
        messages.push(FromClient::Cells(current));
    }
    messages
}

/// The bytes of a `Cells` message before its runs: its tag and their
/// number.
const CELLS_HEADER: usize = 3;

/// `run` in parts that each take at most `room` bytes.
fn split(run: Run, room: usize) -> Vec<Run> {
    if wire::run_length(&run) <= room {
        return vec![run];
    }
    let mut parts = Vec::new();
    let mut part = Run {
        cells: Vec::new(),
        ..run.clone()
    };
    let mut column = run.column;
    for cell in run.cells {
        let length = wire::run_length(&part) + 1 + cell.len();
        if !part.cells.is_empty() && length > room {
            let next = Run {
                column,
                cells: Vec::new(),
                ..part.clone()
            };
            parts.push(std::mem::replace(&mut part, next));
        }
        part.cells.push(cell);
        column = column.saturating_add(1);
    }
    parts.push(part);
    parts
}

#[cfg(test)]
mod tests;
