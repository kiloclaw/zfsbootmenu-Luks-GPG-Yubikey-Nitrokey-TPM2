//! Running the menu on a real terminal.
//!
//! This is the only impure part of the menu: it puts the terminal in raw
//! mode, reads key presses, calls [`update`], and draws with [`view`] until
//! a [`Request`] ends the menu. It also times each frame, so that the pilot
//! can tell how fast the menu redraws on a serial console.
//!
//! Serial consoles often report a size of 0×0, send no resize events, and
//! have nobody on the other end to answer questions: the menu draws into a
//! viewport of a known size ([`Options::size`] when the terminal reports
//! none) and never asks the terminal where its cursor is.

use std::fmt;
use std::io;
use std::time::{Duration, Instant};

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::terminal;
use ratatui::layout::{Position, Rect, Size};
use ratatui::{Terminal, TerminalOptions, Viewport};
use zbm_app::Snapshot;

use crate::keymap;
use crate::model::Model;
use crate::quiet::QuietBackend;
use crate::update::{Request, update};
use crate::view::view;

/// The size assumed when neither the terminal nor [`Options`] give one.
pub const FALLBACK_SIZE: (u16, u16) = (80, 24);

/// How to run the menu.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Options {
    /// Draw with ASCII characters only.
    pub ascii: bool,
    /// The size in columns and rows, when the terminal does not report it.
    pub size: Option<(u16, u16)>,
}

/// How long frames took to draw.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    /// Frames drawn.
    pub frames: u32,
    /// All frames together.
    pub total: Duration,
    /// The slowest frame.
    pub slowest: Duration,
}

impl Stats {
    /// Adds a frame that took `duration`.
    fn record(&mut self, duration: Duration) {
        self.frames = self.frames.saturating_add(1);
        self.total = self.total.saturating_add(duration);
        self.slowest = self.slowest.max(duration);
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mean = self
            .total
            .checked_div(self.frames.max(1))
            .unwrap_or_default();
        write!(
            formatter,
            "{} frames, mean {:.1} ms, slowest {:.1} ms",
            self.frames,
            mean.as_secs_f64() * 1000.0,
            self.slowest.as_secs_f64() * 1000.0
        )
    }
}

/// Leaves raw mode when dropped, so the terminal is usable again however the
/// menu ends.
struct RawMode;

impl RawMode {
    fn enter() -> io::Result<RawMode> {
        terminal::enable_raw_mode()?;
        Ok(RawMode)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

/// The size to draw in: the terminal's, then `options.size`, then
/// [`FALLBACK_SIZE`].
fn size(options: Options) -> (u16, u16) {
    let reported = terminal::size()
        .ok()
        .filter(|(columns, rows)| *columns > 0 && *rows > 0);
    reported.or(options.size).unwrap_or(FALLBACK_SIZE)
}

/// Shows the menu for `snapshot` on standard output until the user boots an
/// environment or leaves; returns the request that ended it and the frame
/// timings.
///
/// # Errors
///
/// When the terminal cannot be set up, read or drawn on.
#[allow(
    clippy::disallowed_methods,
    reason = "the terminal loop is the menu's adapter; it times frames on the wall clock"
)]
pub fn run(snapshot: &Snapshot, options: Options) -> io::Result<(Request, Stats)> {
    let (columns, rows) = size(options);
    let _raw = RawMode::enter()?;
    let backend = QuietBackend::new(
        CrosstermBackend::new(io::stdout()),
        Size::new(columns, rows),
    );
    let viewport = Viewport::Fixed(Rect::new(0, 0, columns, rows));
    let mut terminal = Terminal::with_options(backend, TerminalOptions { viewport })?;
    terminal.clear()?;
    let mut model = Model::new(snapshot, options.ascii);
    let mut stats = Stats::default();
    let request = loop {
        let start = Instant::now();
        terminal.draw(|frame| view(&model, frame))?;
        stats.record(start.elapsed());
        let message = match event::read()? {
            Event::Key(key) => keymap::message(key),
            Event::Resize(columns, rows) => {
                terminal.backend_mut().set_size(Size::new(columns, rows));
                terminal.resize(Rect::new(0, 0, columns, rows))?;
                None
            }
            _ => None,
        };
        let Some(message) = message else {
            continue;
        };
        let (next, requests) = update(model, message);
        model = next;
        if let Some(request) = requests.into_iter().next() {
            break request;
        }
    };
    terminal.clear()?;
    terminal.set_cursor_position(Position::ORIGIN)?;
    terminal.show_cursor()?;
    Ok((request, stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_report_the_mean_and_the_slowest_frame() {
        let mut stats = Stats::default();
        stats.record(Duration::from_millis(2));
        stats.record(Duration::from_millis(4));

        assert_eq!(stats.to_string(), "2 frames, mean 3.0 ms, slowest 4.0 ms");
    }

    #[test]
    fn no_frames_have_a_mean_of_zero() {
        assert_eq!(
            Stats::default().to_string(),
            "0 frames, mean 0.0 ms, slowest 0.0 ms"
        );
    }
}
