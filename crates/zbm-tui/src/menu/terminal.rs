//! Running the menu on a real terminal: raw mode, key presses and ticks in,
//! frames out, until the menu ends. The only impure part of the menu.

use std::fs::File;
use std::io::{self, Write as _};
use std::time::{Duration, Instant};

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::terminal;
use ratatui::layout::{Position, Rect, Size};
use ratatui::{Terminal, TerminalOptions, Viewport};

use super::keymap::{Modifiers, key_name};
use super::machine::Machine;
use super::secret::{Marker, is_asking_secret};
use super::session::Session;
use super::startup::{Keys as _, StartupScreen, StartupView};
use super::term_keys::TerminalKeys;
use super::theme::Theme;
use super::view::view;
use super::world::Snapshot;
use crate::quiet::QuietBackend;

/// The terminal the menu draws on.
type Screen = Terminal<QuietBackend<CrosstermBackend<io::Stdout>>>;

/// How often time passes for the menu: the countdown and start-up advance.
const TICK: Duration = Duration::from_millis(200);

/// How long the last frame stays before the menu ends.
const LAST_FRAME: Duration = Duration::from_millis(1500);

/// Runs `session` on standard output; returns why the menu ended. With
/// `keys`, every key press is logged there with the milliseconds since
/// start, for the usability sessions. The keys are read as the start-up
/// view reads them, the rest of a key's sequence taking up to
/// `escape_wait` ([`super::term_keys::CONSOLE_ESCAPE_WAIT`],
/// [`super::term_keys::SERIAL_ESCAPE_WAIT`]), so that a sequence split on
/// a serial console reached through the network is not taken for Escape.
/// Actions that take the console get the terminal as it was before the
/// menu, and the menu is drawn anew after them.
///
/// # Errors
///
/// When the terminal cannot be set up, read or drawn on.
#[allow(
    clippy::disallowed_methods,
    reason = "the terminal loop is the menu's adapter; it measures time on the wall clock"
)]
pub fn run<M: Machine>(
    mut session: Session<M>,
    size: (u16, u16),
    mut keys: Option<File>,
    escape_wait: Duration,
) -> io::Result<String> {
    let mut input = TerminalKeys::new(escape_wait)?;
    let (columns, rows) = size;
    let backend = QuietBackend::new(
        CrosstermBackend::new(io::stdout()),
        Size::new(columns, rows),
    );
    let viewport = Viewport::Fixed(Rect::new(0, 0, columns, rows));
    let mut terminal = Terminal::with_options(backend, TerminalOptions { viewport })?;
    terminal.clear()?;
    let start = Instant::now();
    let mut last_tick = Instant::now();
    // The console on the display keeps nothing while a secret is typed.
    let mut marker = Marker::new(std::path::Path::new(zbm_state::BASE));
    let why = loop {
        fit(&mut terminal, &mut input)?;
        terminal.draw(|frame| view(session.model(), frame))?;
        marker.show(is_asking_secret(session.model()));
        if let Some(why) = session.leaving() {
            // The last frame stays a moment, unless it says nothing.
            if !why.is_empty() {
                std::thread::sleep(LAST_FRAME);
            }
            break why.to_owned();
        }
        let wait = TICK.saturating_sub(last_tick.elapsed());
        if let Some(key) = input.key(wait)? {
            if let Some(file) = &mut keys {
                let name = key_name(key.key, Modifiers::of(&key));
                writeln!(file, "{} {name}", start.elapsed().as_millis())?;
            }
            session.press(key);
        }
        if last_tick.elapsed() >= TICK {
            let elapsed = u64::try_from(last_tick.elapsed().as_millis()).unwrap_or(u64::MAX);
            session.tick(elapsed);
            last_tick = Instant::now();
        }
        hand_over(&mut session, &mut terminal, &mut input)?;
    };
    give_back(&mut terminal)?;
    Ok(why)
}

/// Follows the terminal's size, when it changed since the last frame.
fn fit(terminal: &mut Screen, input: &mut TerminalKeys) -> io::Result<()> {
    let Some((columns, rows)) = input.size().filter(|&(c, r)| c > 0 && r > 0) else {
        return Ok(());
    };
    let area = Rect::new(0, 0, columns, rows);
    if terminal.get_frame().area() != area {
        terminal.backend_mut().set_size(Size::new(columns, rows));
        terminal.resize(area)?;
    }
    Ok(())
}

/// Leaves the terminal as it was before the menu: cleared, with the cursor
/// at the top and shown.
fn give_back(terminal: &mut Screen) -> io::Result<()> {
    terminal.clear()?;
    terminal.set_cursor_position(Position::ORIGIN)?;
    terminal.show_cursor()
}

/// Performs the actions of `session` that take the console, each with the
/// terminal out of raw mode; the next frame is drawn whole.
fn hand_over<M: Machine>(
    session: &mut Session<M>,
    terminal: &mut Screen,
    input: &mut TerminalKeys,
) -> io::Result<()> {
    while let Some(action) = session.console_action() {
        give_back(terminal)?;
        input.release()?;
        session.perform(action);
        input.take()?;
        terminal.clear()?;
    }
    Ok(())
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
    let keys = TerminalKeys::new(escape_wait)?;
    let (columns, rows) = size;
    let backend = QuietBackend::new(
        CrosstermBackend::new(io::stdout()),
        Size::new(columns, rows),
    );
    let viewport = Viewport::Fixed(Rect::new(0, 0, columns, rows));
    let terminal = Terminal::with_options(backend, TerminalOptions { viewport })?;
    let marker = Marker::new(std::path::Path::new(zbm_state::BASE));
    Ok(Box::new(
        StartupView::new(terminal, keys, snapshot, theme)?.with_marker(marker),
    ))
}

/// The size to draw in: the terminal's, else `fallback`.
pub fn size(fallback: (u16, u16)) -> (u16, u16) {
    terminal::size()
        .ok()
        .filter(|(columns, rows)| *columns > 0 && *rows > 0)
        .unwrap_or(fallback)
}
