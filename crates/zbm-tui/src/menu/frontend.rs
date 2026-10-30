//! The one runtime of the menu's front ends (ROADMAP.md, UI3; ADR 0004):
//! [`Frontend`] draws a [`Model`] on a [`Surface`] and reads keys from an
//! [`Input`], whatever they are: a terminal (the console, a serial line,
//! SSH), the pixels of the display, or Ratatui's test backend with scripted
//! keys. It follows the surface's size, counts time on a [`Clock`], records
//! the secret prompts for the console on the display, logs the keys of the
//! usability sessions, lends the terminal to the programs that need it, and
//! keeps the last frame a moment. The menu ([`Frontend::run`]), the demo and
//! the start-up view ([`StartupView`](super::startup::StartupView)) are
//! built on it, so the model, the view and the key map are the same on
//! all of them.

use std::fs::File;
use std::io::{self, Write as _};
use std::os::fd::BorrowedFd;
use std::time::Duration;

use ratatui::backend::{Backend, TestBackend};
use ratatui::layout::{Position, Rect, Size};
use ratatui::{Terminal, TerminalOptions, Viewport};
use zbm_display::keys::KeyPress;
use zbm_platform::clock::{Clock, SystemClock};

use super::keymap::{Modifiers, key_name};
use super::machine::Machine;
use super::model::Model;
use super::secret::{Marker, is_asking_secret};
use super::session::Session;
use super::view::view;
use crate::quiet::QuietBackend;

/// How often time passes for the menu: the countdown, the dialogs' time and
/// start-up advance.
pub const TICK: Duration = Duration::from_millis(200);

/// How long the last frame stays before the menu ends.
pub const LAST_FRAME: Duration = Duration::from_millis(1500);

/// Where the keys come from, and the size of what they are typed at.
pub trait Input {
    /// The next key pressed within `timeout`, and no longer once `wake` is
    /// readable, as when the machine sent something: `None` when none
    /// came. An input that cannot wait on `wake` waits for `timeout`.
    ///
    /// # Errors
    ///
    /// When the input cannot be read.
    fn key_or_wake(
        &mut self,
        timeout: Duration,
        wake: Option<BorrowedFd<'_>>,
    ) -> io::Result<Option<KeyPress>>;

    /// The next key pressed within `timeout`.
    ///
    /// # Errors
    ///
    /// When the input cannot be read.
    fn key(&mut self, timeout: Duration) -> io::Result<Option<KeyPress>> {
        self.key_or_wake(timeout, None)
    }

    /// Gives the input back to programs, as it was before the front end.
    ///
    /// # Errors
    ///
    /// When its mode cannot be set.
    fn release(&mut self) -> io::Result<()> {
        Ok(())
    }

    /// Takes it again, after [`Input::release`].
    ///
    /// # Errors
    ///
    /// When its mode cannot be set.
    fn take(&mut self) -> io::Result<()> {
        Ok(())
    }

    /// The size now, in columns and rows, if it can be measured and can
    /// change: a terminal whose font a hook changed, or whose window was
    /// resized.
    fn size(&mut self) -> Option<(u16, u16)> {
        None
    }
}

/// What the menu is drawn on: a Ratatui backend that is told its new size,
/// and shows each frame once it is drawn.
pub trait Surface: Backend {
    /// Records the new size, as the input measured it.
    fn set_size(&mut self, size: Size);

    /// Shows the frame just drawn, where drawing does not show it already.
    ///
    /// # Errors
    ///
    /// When it cannot be shown.
    fn present(&mut self) -> io::Result<()> {
        Ok(())
    }

    /// Says whether the keys that follow belong to a secret, where the
    /// surface carries the flag with its frames (the console's cells).
    fn set_secret(&mut self, secret: bool) {
        let _ = secret;
    }
}

impl<B: Backend> Surface for QuietBackend<B> {
    fn set_size(&mut self, size: Size) {
        QuietBackend::set_size(self, size);
    }
}

impl Surface for TestBackend {
    fn set_size(&mut self, size: Size) {
        self.resize(size.width, size.height);
    }
}

/// A result of a surface, with its error as an I/O error.
fn drawn<T, E>(result: Result<T, E>) -> io::Result<T>
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    result.map_err(io::Error::other)
}

/// A front end: a surface, its input and its time.
pub struct Frontend<B: Surface, I: Input> {
    terminal: Terminal<B>,
    input: I,
    /// Where secret prompts are recorded for the console on the display.
    marker: Option<Marker>,
    /// The clock the menu's time passes on.
    clock: Box<dyn Clock>,
    /// Where the keys are logged, with when the front end started.
    log: Option<(File, Duration)>,
}

impl<B: Surface, I: Input> Frontend<B, I>
where
    B::Error: Send + Sync + 'static,
{
    /// A front end drawing on `surface`, of the size it says, and reading
    /// `input`; the surface is cleared.
    ///
    /// # Errors
    ///
    /// When the surface cannot be measured or cleared.
    pub fn new(surface: B, input: I) -> io::Result<Frontend<B, I>> {
        let size = drawn(surface.size())?;
        let viewport = Viewport::Fixed(Rect::new(0, 0, size.width, size.height));
        let mut terminal = drawn(Terminal::with_options(
            surface,
            TerminalOptions { viewport },
        ))?;
        drawn(terminal.clear())?;
        Ok(Frontend {
            terminal,
            input,
            marker: None,
            clock: Box::new(SystemClock::new()),
            log: None,
        })
    }

    /// The front end, which records its secret prompts with `marker`.
    #[must_use]
    pub fn with_marker(mut self, marker: Marker) -> Frontend<B, I> {
        self.marker = Some(marker);
        self
    }

    /// The front end, whose time passes on `clock`.
    #[must_use]
    pub fn with_clock(mut self, clock: impl Clock + 'static) -> Frontend<B, I> {
        self.clock = Box::new(clock);
        self
    }

    /// The front end, which logs every key to `file` with the milliseconds
    /// since it started, for the usability sessions.
    #[must_use]
    pub fn with_key_log(mut self, file: File) -> Frontend<B, I> {
        self.log = Some((file, self.clock.now()));
        self
    }

    /// The terminal drawn on, for the tests that read what it shows.
    pub fn terminal(&self) -> &Terminal<B> {
        &self.terminal
    }

    /// The time on the front end's clock.
    pub fn now(&self) -> Duration {
        self.clock.now()
    }

    /// Follows the input's size, when it changed since the last frame.
    fn fit(&mut self) -> io::Result<()> {
        let Some((columns, rows)) = self.input.size().filter(|&(c, r)| c > 0 && r > 0) else {
            return Ok(());
        };
        let area = Rect::new(0, 0, columns, rows);
        if self.terminal.get_frame().area() == area {
            return Ok(());
        }
        self.terminal
            .backend_mut()
            .set_size(Size::new(columns, rows));
        drawn(self.terminal.resize(area))
    }

    /// Draws `model` and shows it.
    ///
    /// # Errors
    ///
    /// When the surface cannot be drawn on.
    pub fn draw(&mut self, model: &Model) -> io::Result<()> {
        self.fit()?;
        drawn(self.terminal.draw(|frame| view(model, frame)))?;
        let secret = is_asking_secret(model);
        if let Some(marker) = &mut self.marker {
            marker.show(secret);
        }
        let surface = self.terminal.backend_mut();
        surface.set_secret(secret);
        surface.present()
    }

    /// The next key within `timeout`, or until `wake` is readable; logged
    /// when the front end logs keys.
    ///
    /// # Errors
    ///
    /// When the input cannot be read, or the log written.
    pub fn key(
        &mut self,
        timeout: Duration,
        wake: Option<BorrowedFd<'_>>,
    ) -> io::Result<Option<KeyPress>> {
        let key = self.input.key_or_wake(timeout, wake)?;
        if let (Some(key), Some((file, start))) = (key, &mut self.log) {
            let since = self.clock.now().saturating_sub(*start).as_millis();
            let name = key_name(key.key, Modifiers::of(&key));
            writeln!(file, "{since} {name}")?;
        }
        Ok(key)
    }

    /// Leaves the surface as it was before the front end: cleared, with
    /// the cursor at the top and shown, and gives the input back.
    ///
    /// # Errors
    ///
    /// When the surface or the input cannot be given back.
    pub fn give_back(&mut self) -> io::Result<()> {
        if let Some(marker) = &mut self.marker {
            marker.show(false);
        }
        self.terminal.backend_mut().set_secret(false);
        drawn(self.terminal.clear())?;
        drawn(self.terminal.set_cursor_position(Position::ORIGIN))?;
        drawn(self.terminal.show_cursor())?;
        self.input.release()
    }

    /// Takes the surface and the input again after [`Frontend::give_back`];
    /// the next frame is drawn whole.
    ///
    /// # Errors
    ///
    /// When they cannot be taken.
    pub fn take_back(&mut self) -> io::Result<()> {
        self.input.take()?;
        drawn(self.terminal.clear())
    }

    /// Keeps the last frame a moment: keys pressed meanwhile are lost.
    fn last_frame(&mut self) -> io::Result<()> {
        let end = self.clock.now().saturating_add(LAST_FRAME);
        loop {
            let left = end.saturating_sub(self.clock.now());
            if left.is_zero() || self.input.key(left)?.is_none() {
                return Ok(());
            }
        }
    }

    /// Runs `session`: frames out, keys and time in, and what the machine
    /// sends, which wakes the front end, until the menu ends; why it ended.
    /// Actions that take the console get it as it was before the menu, and
    /// the menu is drawn anew after them.
    ///
    /// # Errors
    ///
    /// When the surface cannot be drawn on or the input read.
    pub fn run<M: Machine>(&mut self, session: &mut Session<M>) -> io::Result<String> {
        let mut last_tick = self.clock.now();
        loop {
            self.draw(session.model())?;
            if let Some(why) = session.leaving() {
                let why = why.to_owned();
                // The last frame stays a moment, unless it says nothing.
                if !why.is_empty() {
                    self.last_frame()?;
                }
                self.give_back()?;
                return Ok(why);
            }
            let passed = self.clock.now().saturating_sub(last_tick);
            let key = self.key(TICK.saturating_sub(passed), session.waker())?;
            session.poll();
            if let Some(key) = key {
                session.press(key);
            }
            let passed = self.clock.now().saturating_sub(last_tick);
            if passed >= TICK {
                last_tick = self.clock.now();
                session.tick(u64::try_from(passed.as_millis()).unwrap_or(u64::MAX));
            }
            while let Some(action) = session.console_action() {
                self.give_back()?;
                session.perform(action);
                self.take_back()?;
            }
        }
    }
}

#[cfg(test)]
mod tests;
