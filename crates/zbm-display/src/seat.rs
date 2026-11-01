//! The seat: the display and the virtual terminal, held or handed back, as
//! typestate (ADR 0004, rule 5; ROADMAP.md UI5). Only a held seat gives the
//! screen to draw on, so nothing draws on a display handed back before
//! `kexec`: `Seat<Held>` → [`Seat::release`] → `Seat<Released>` →
//! [`Seat::take`] again, as `zbm console reattach` asks after a failed
//! `kexec`.
//!
//! The keyboards are not the seat's: they come and go as they are plugged
//! in, and the loop that reads them owns them.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::devices::{Kind, Opener, direct};
use crate::display::{Display, Options, Screen};
use crate::vt::Terminal;

/// A seat held: the display shown, and the virtual terminal taken over.
#[derive(Debug)]
pub struct Held {
    display: Display,
    /// `None` without a virtual terminal, on a serial console only.
    terminal: Option<Terminal>,
}

/// A seat handed back: the kernel console has the display and the
/// terminal.
#[derive(Debug)]
pub struct Released;

/// The display of a DRM card and a virtual terminal, held or handed back.
#[derive(Debug)]
pub struct Seat<S> {
    card: PathBuf,
    terminal: Option<PathBuf>,
    options: Options,
    /// What opens the card and the terminal (ADR 0012).
    opener: Arc<dyn Opener>,
    state: S,
}

impl<S> Seat<S> {
    /// The DRM card.
    pub fn card(&self) -> &Path {
        &self.card
    }
}

impl Seat<Released> {
    /// The seat of the DRM card `card` and the virtual terminal `terminal`,
    /// not taken yet.
    pub fn new(card: PathBuf, terminal: Option<PathBuf>) -> Seat<Released> {
        Seat {
            card,
            terminal,
            options: Options::default(),
            opener: direct(),
            state: Released,
        }
    }

    /// The seat, its card and terminal opened by `opener`.
    #[must_use]
    pub fn with_opener(mut self, opener: Arc<dyn Opener>) -> Seat<Released> {
        self.opener = opener;
        self
    }

    /// The seat, showing its display with `options`.
    #[must_use]
    pub fn with_options(mut self, options: Options) -> Seat<Released> {
        self.options = options;
        self
    }

    /// Takes the display, then the virtual terminal.
    ///
    /// # Errors
    ///
    /// When either cannot be had: the seat back, with why; what was taken
    /// is handed back.
    pub fn take(self) -> Result<Seat<Held>, (Seat<Released>, io::Error)> {
        let display = match self
            .opener
            .open(Kind::Card, &self.card)
            .and_then(|file| Display::of_card(file, self.options))
        {
            Ok(display) => display,
            Err(error) => return Err((self, error)),
        };
        let terminal = self.terminal.as_deref().map(|path| {
            self.opener
                .open(Kind::Terminal, path)
                .and_then(Terminal::take_file)
        });
        let terminal = match terminal.transpose() {
            Ok(terminal) => terminal,
            Err(error) => {
                drop(display);
                return Err((self, error));
            }
        };
        Ok(Seat {
            card: self.card,
            terminal: self.terminal,
            options: self.options,
            opener: self.opener,
            state: Held { display, terminal },
        })
    }
}

impl Seat<Held> {
    /// The screen to draw on.
    pub fn screen(&mut self) -> &mut Display {
        &mut self.state.display
    }

    /// The display's size in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.state.display.size()
    }

    /// Hands the seat back: the output to the kernel console first, then
    /// the virtual terminal, in text mode with its keyboard.
    pub fn release(self) -> Seat<Released> {
        let Held { display, terminal } = self.state;
        drop(display);
        drop(terminal);
        Seat {
            card: self.card,
            terminal: self.terminal,
            options: self.options,
            opener: self.opener,
            state: Released,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seat_that_cannot_be_taken_comes_back_with_why() {
        let seat = Seat::new(PathBuf::from("/nonexistent/card0"), None);
        let (seat, error) = seat.take().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
        assert_eq!(seat.card(), Path::new("/nonexistent/card0"));
        assert!(format!("{seat:?}").contains("Released"));
    }
}
