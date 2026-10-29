//! Running the menu on a real terminal: raw mode, key presses and ticks in,
//! frames out, until the menu ends; the [`Frontend`] on a terminal. The only
//! impure part of the menu.

use std::fs::File;
use std::io;
use std::path::Path;
use std::time::Duration;

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::terminal;
use ratatui::layout::Size;

use super::frontend::Frontend;
use super::machine::Machine;
use super::screen::{MenuScreen, SessionScreen};
use super::secret::Marker;
use super::session::Session;
use super::startup::{StartupScreen, StartupView};
use super::term_keys::TerminalKeys;
use super::theme::Theme;
use super::world::Snapshot;
use crate::quiet::QuietBackend;

/// The front end on standard output and its keys, of `size` (columns and
/// rows) until the terminal says otherwise, the rest of a key's sequence
/// taking up to `escape_wait`.
fn on_terminal(
    size: (u16, u16),
    escape_wait: Duration,
) -> io::Result<Frontend<QuietBackend<CrosstermBackend<io::Stdout>>, TerminalKeys>> {
    let keys = TerminalKeys::new(escape_wait)?;
    let (columns, rows) = size;
    let backend = QuietBackend::new(
        CrosstermBackend::new(io::stdout()),
        Size::new(columns, rows),
    );
    // The console on the display keeps nothing while a secret is typed.
    let marker = Marker::new(Path::new(zbm_app::console::SOCKET));
    Ok(Frontend::new(backend, keys)?.with_marker(marker))
}

/// Runs `session` on standard output; returns why the menu ended. With
/// `keys`, every key press is logged there with the milliseconds since
/// start, for the usability sessions. The keys are read as the start-up
/// view reads them, the rest of a key's sequence taking up to
/// `escape_wait` ([`super::term_keys::CONSOLE_ESCAPE_WAIT`],
/// [`super::term_keys::SERIAL_ESCAPE_WAIT`]), so that a sequence split on
/// a serial console reached through the network is not taken for Escape
/// ([`Frontend::run`]).
///
/// # Errors
///
/// When the terminal cannot be set up, read or drawn on.
pub fn run<M: Machine>(
    session: Session<M>,
    size: (u16, u16),
    keys: Option<File>,
    escape_wait: Duration,
) -> io::Result<String> {
    let mut frontend = on_terminal(size, escape_wait)?;
    if let Some(file) = keys {
        frontend = frontend.with_key_log(file);
    }
    let mut session = session;
    frontend.run(&mut session)
}

/// The start-up view of `snapshot` on standard output, of `size` (columns
/// and rows) until the terminal says otherwise, with the keys of the
/// terminal, the rest of whose sequences may take `escape_wait`
/// ([`super::term_keys::CONSOLE_ESCAPE_WAIT`],
/// [`super::term_keys::SERIAL_ESCAPE_WAIT`]).
///
/// # Errors
///
/// When the terminal cannot be set up or drawn on.
pub fn startup_view(
    snapshot: Snapshot,
    theme: Theme,
    size: (u16, u16),
    escape_wait: Duration,
) -> io::Result<Box<dyn StartupScreen>> {
    let frontend = on_terminal(size, escape_wait)?;
    Ok(Box::new(StartupView::new(frontend, snapshot, theme)?))
}

/// The menu's screen in the session, on standard output, without the
/// start-up view: as it opens after `--resume`. Its keys and size as
/// [`startup_view`]'s; the terminal is the console's until the menu takes
/// it.
///
/// # Errors
///
/// When the terminal cannot be set up.
pub fn menu_screen(
    theme: Theme,
    size: (u16, u16),
    escape_wait: Duration,
) -> io::Result<Box<dyn MenuScreen>> {
    let mut frontend = on_terminal(size, escape_wait)?;
    frontend.give_back()?;
    Ok(Box::new(SessionScreen::new(frontend, None, theme)))
}

/// The size to draw in: the terminal's, else `fallback`.
pub fn size(fallback: (u16, u16)) -> (u16, u16) {
    terminal::size()
        .ok()
        .filter(|(columns, rows)| *columns > 0 && *rows > 0)
        .unwrap_or(fallback)
}
