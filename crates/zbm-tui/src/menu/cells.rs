//! The menu on the console's cell surface (ADR 0004, UI6): on the display,
//! the menu sends the console the cells it draws, and gets the keys pressed,
//! on the client channel the supervisor passed it ([`CELLS_FD`]), instead of
//! writing escape sequences to a terminal that the console parses back.
//!
//! [`CellSurface`] is the Ratatui backend: the cells of a frame go in runs
//! of one style, as many packets as they take, and its cursor ends it, so
//! that the console shows the frame whole. [`CellKeys`] is its input: the
//! keys and the new sizes the console sends. Giving the input back lends
//! the screen to the programs' terminal, as for a shell, and taking it takes
//! the screen again. Without a channel, or a console that answers its
//! hello in time and in this version, the menu draws on its terminal.

use std::io;
use std::os::fd::{AsFd as _, BorrowedFd};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use ratatui::text::Span;
use zbm_display::channel::{self, Caps, FromClient, FromConsole, Run, VERSION};
use zbm_display::grid::CellStyle;
use zbm_display::keys::KeyPress;
use zbm_sys::console::{Watch, wait_ready};
use zbm_sys::socket::{Connection, MAX_MESSAGE};

use super::display::cell_style;
use super::frontend::{Input, Surface};

/// The variable that names the descriptor of the menu's end of the client
/// channel, which the supervisor passes the start-up program.
pub const CELLS_FD: &str = "ZBM_CELLS_FD";

/// How long the console has to answer the menu's hello.
pub const HANDSHAKE: Duration = Duration::from_secs(1);

/// How long a frame waits for room in the channel before drawing fails:
/// a console that reads nothing does not hold the menu up for good.
pub const SEND_WAIT: Duration = Duration::from_secs(2);

/// The channel of this process: the descriptor `number`, which its parent
/// passed, taken the first time it is asked for, and kept for the front
/// ends that follow.
fn inherited(number: &str) -> Option<Arc<Connection>> {
    static CHANNEL: Mutex<Option<Arc<Connection>>> = Mutex::new(None);
    let mut channel = CHANNEL.lock().unwrap_or_else(PoisonError::into_inner);
    if channel.is_none() {
        let connection = Connection::inherited(number.parse().ok()?).ok()?;
        connection.set_send_timeout(SEND_WAIT).ok()?;
        *channel = Some(Arc::new(connection));
    }
    channel.clone()
}

/// The surface and the keys of the console's cells on the channel whose
/// descriptor `number` names, which take the screen; `None` without a
/// channel, or when the console does not answer in time or in this
/// version.
pub fn connect(number: Option<&str>) -> Option<(CellSurface, CellKeys)> {
    let channel = inherited(number?)?;
    let caps = handshake(&channel, HANDSHAKE).ok()??;
    let mut keys = CellKeys::new(Arc::clone(&channel), caps);
    keys.take().ok()?;
    Some((CellSurface::new(channel, caps), keys))
}

/// Says hello on `channel` and waits at most `wait` for the console's hello
/// and what it shows; `None` when it does not answer in time or speaks
/// another version. What a client before this one left unread is dropped
/// first.
///
/// # Errors
///
/// When the channel cannot be read or written.
#[allow(
    clippy::disallowed_methods,
    reason = "the channel is the console's adapter; its answer is awaited in wall-clock time"
)]
pub fn handshake(channel: &Connection, wait: Duration) -> io::Result<Option<Caps>> {
    while channel
        .receive(Some(Duration::ZERO))?
        .is_some_and(|packet| !packet.is_empty())
    {}
    channel.send(&FromClient::Hello { version: VERSION }.encode())?;
    let until = std::time::Instant::now() + wait;
    let mut is_greeted = false;
    loop {
        let left = until.saturating_duration_since(std::time::Instant::now());
        let Some(packet) = channel.receive(Some(left))? else {
            return Ok(None);
        };
        if packet.is_empty() {
            return Ok(None);
        }
        match FromConsole::decode(&packet) {
            Some(FromConsole::Hello { version }) if version == VERSION => is_greeted = true,
            Some(FromConsole::Hello { .. }) => return Ok(None),
            Some(FromConsole::Caps(caps)) if is_greeted => return Ok(Some(caps)),
            _ => {}
        }
    }
}

/// The console's cells as a Ratatui backend: what a frame draws is kept in
/// runs until it is presented.
#[derive(Debug)]
pub struct CellSurface {
    channel: Arc<Connection>,
    size: Size,
    cursor: Position,
    is_cursor_shown: bool,
    /// The cells drawn since the last frame was presented.
    runs: Vec<Run>,
    /// Whether the console was told that a secret is typed.
    is_secret: bool,
}

impl CellSurface {
    /// The cells on `channel`, as many as `caps` says.
    pub fn new(channel: Arc<Connection>, caps: Caps) -> CellSurface {
        CellSurface {
            channel,
            size: Size::new(caps.columns, caps.rows),
            cursor: Position::ORIGIN,
            is_cursor_shown: false,
            runs: Vec::new(),
            is_secret: false,
        }
    }

    /// Adds the cell `text` at (`x`, `y`) to the runs, at the end of the
    /// last when it follows it in the same style.
    fn push(&mut self, (x, y): (u16, u16), style: CellStyle, text: &str) {
        if let Some(last) = self.runs.last_mut() {
            let end = usize::from(last.column) + last.cells.len();
            if last.row == y && last.style == style && end == usize::from(x) {
                last.cells.push(text.to_owned());
                return;
            }
        }
        self.runs.push(Run {
            row: y,
            column: x,
            style,
            cells: vec![text.to_owned()],
        });
    }

    /// Sends `message` to the console.
    fn send(&self, message: &FromClient) -> io::Result<()> {
        self.channel.send(&message.encode())
    }
}

impl Backend for CellSurface {
    type Error = io::Error;

    /// Keeps the cells of `content`; a wide character's second cell is an
    /// empty one, as the console takes it.
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            let style = cell_style(cell);
            let symbol = cell.symbol();
            self.push((x, y), style, symbol);
            if Span::raw(symbol).width() > 1 {
                self.push((x.saturating_add(1), y), style, "");
            }
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.is_cursor_shown = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.is_cursor_shown = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.cursor = position.into();
        Ok(())
    }

    /// Blanks every cell, in the next frame: what was drawn and not yet
    /// presented goes.
    fn clear(&mut self) -> io::Result<()> {
        self.runs.clear();
        let blank = cell_style(&Cell::default());
        for row in 0..self.size.height {
            self.runs.push(Run {
                row,
                column: 0,
                style: blank,
                cells: vec![" ".to_owned(); usize::from(self.size.width)],
            });
        }
        Ok(())
    }

    fn clear_region(&mut self, _: ClearType) -> io::Result<()> {
        self.clear()
    }

    fn size(&self) -> io::Result<Size> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.size,
            pixels: Size::default(),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Surface for CellSurface {
    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    /// Sends the frame's cells, then its cursor, which ends it.
    fn present(&mut self) -> io::Result<()> {
        let runs = std::mem::take(&mut self.runs);
        for message in channel::cells_messages(runs, MAX_MESSAGE) {
            self.send(&message)?;
        }
        let cursor = self
            .is_cursor_shown
            .then_some((self.cursor.x, self.cursor.y));
        self.send(&FromClient::Cursor(cursor))
    }

    /// Tells the console, in the same stream as the frames, whether the
    /// keys that follow belong to a secret.
    fn set_secret(&mut self, secret: bool) {
        if secret != self.is_secret && self.send(&FromClient::Secret(secret)).is_ok() {
            self.is_secret = secret;
        }
    }
}

/// The keys the console sends, and its size.
#[derive(Debug)]
pub struct CellKeys {
    channel: Arc<Connection>,
    size: (u16, u16),
}

impl CellKeys {
    /// The keys on `channel`, of a console of the size `caps` says.
    pub fn new(channel: Arc<Connection>, caps: Caps) -> CellKeys {
        CellKeys {
            channel,
            size: (caps.columns, caps.rows),
        }
    }

    /// The next message, if one came: a key, or nothing but the size taken.
    ///
    /// # Errors
    ///
    /// When the channel cannot be read, or the console is gone.
    fn read(&mut self) -> io::Result<Option<KeyPress>> {
        let Some(mut packet) = self.channel.receive(Some(Duration::ZERO))? else {
            return Ok(None);
        };
        if packet.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the console on the display is gone",
            ));
        }
        let message = FromConsole::decode(&packet);
        // It may be a key of a passphrase.
        zbm_sys::wipe_vec(&mut packet);
        Ok(match message {
            Some(FromConsole::Key { press, .. }) => Some(press),
            Some(FromConsole::Resize { columns, rows }) => {
                self.size = (columns, rows);
                None
            }
            _ => None,
        })
    }

    /// Sends `message` to the console.
    fn send(&self, message: &FromClient) -> io::Result<()> {
        self.channel.send(&message.encode())
    }
}

impl Input for CellKeys {
    /// The next key within `timeout`, and no longer once `wake` is
    /// readable, or another message came.
    fn key_or_wake(
        &mut self,
        timeout: Duration,
        wake: Option<BorrowedFd<'_>>,
    ) -> io::Result<Option<KeyPress>> {
        let mut watches = vec![Watch::read(self.channel.as_fd())];
        watches.extend(wake.map(Watch::read));
        let ready = wait_ready(&watches, Some(timeout))?;
        if ready.first().is_some_and(|channel| channel.is_ready()) {
            self.read()
        } else {
            Ok(None)
        }
    }

    /// Lends the screen to the programs' terminal.
    fn release(&mut self) -> io::Result<()> {
        self.send(&FromClient::Lend)
    }

    /// Takes the screen again, blank until the next frame.
    fn take(&mut self) -> io::Result<()> {
        self.send(&FromClient::Take)
    }

    fn size(&mut self) -> Option<(u16, u16)> {
        Some(self.size)
    }
}

#[cfg(test)]
mod tests;
