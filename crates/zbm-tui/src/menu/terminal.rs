//! Running the menu on a real terminal: raw mode, key presses and ticks in,
//! frames out, until the menu ends. The only impure part of the menu.

use std::fs::File;
use std::io::{self, Write as _};
use std::time::{Duration, Instant};

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::crossterm::terminal;
use ratatui::layout::{Position, Rect, Size};
use ratatui::{Terminal, TerminalOptions, Viewport};

use super::demo::Session;
use super::keymap::key_name;
use super::view::view;
use crate::quiet::QuietBackend;

/// How often time passes for the menu: the countdown and start-up advance.
const TICK: Duration = Duration::from_millis(200);

/// How long the last frame stays before the menu ends.
const LAST_FRAME: Duration = Duration::from_millis(1500);

/// Leaves raw mode when dropped.
struct RawMode;

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

/// Runs `session` on standard output; returns why the menu ended. With
/// `keys`, every key press is logged there with the milliseconds since
/// start, for the usability sessions.
///
/// # Errors
///
/// When the terminal cannot be set up, read or drawn on.
#[allow(
    clippy::disallowed_methods,
    reason = "the terminal loop is the menu's adapter; it measures time on the wall clock"
)]
pub fn run(mut session: Session, size: (u16, u16), mut keys: Option<File>) -> io::Result<String> {
    terminal::enable_raw_mode()?;
    let _raw = RawMode;
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
    let why = loop {
        terminal.draw(|frame| view(session.model(), frame))?;
        if let Some(why) = session.leaving() {
            std::thread::sleep(LAST_FRAME);
            break why.to_owned();
        }
        let wait = TICK.saturating_sub(last_tick.elapsed());
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if let Some(file) = &mut keys {
                        let name = key_name(key.code, key.modifiers);
                        writeln!(file, "{} {name}", start.elapsed().as_millis())?;
                    }
                    session.press(key.code, key.modifiers);
                }
                Event::Resize(columns, rows) => {
                    terminal.backend_mut().set_size(Size::new(columns, rows));
                    terminal.resize(Rect::new(0, 0, columns, rows))?;
                }
                _ => {}
            }
        }
        if last_tick.elapsed() >= TICK {
            let elapsed = u64::try_from(last_tick.elapsed().as_millis()).unwrap_or(u64::MAX);
            session.tick(elapsed);
            last_tick = Instant::now();
        }
    };
    terminal.clear()?;
    terminal.set_cursor_position(Position::ORIGIN)?;
    terminal.show_cursor()?;
    Ok(why)
}

/// The size to draw in: the terminal's, else `fallback`.
pub fn size(fallback: (u16, u16)) -> (u16, u16) {
    terminal::size()
        .ok()
        .filter(|(columns, rows)| *columns > 0 && *rows > 0)
        .unwrap_or(fallback)
}
