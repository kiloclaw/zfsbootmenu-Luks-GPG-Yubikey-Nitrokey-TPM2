//! The modes of a terminal, saved to be restored.

use std::io;
use std::os::fd::{AsFd, OwnedFd};

use rustix::termios::{self, OptionalActions};

/// The modes of a terminal, saved to be restored: a program killed in raw
/// mode leaves its terminal raw, without Ctrl-C or echo.
#[derive(Debug)]
pub struct Modes {
    terminal: OwnedFd,
    saved: termios::Termios,
}

impl Modes {
    /// The modes of `terminal` now; `None` when it is not a terminal.
    pub fn save(terminal: impl AsFd) -> Option<Modes> {
        let saved = termios::tcgetattr(&terminal).ok()?;
        let terminal = terminal.as_fd().try_clone_to_owned().ok()?;
        Some(Modes { terminal, saved })
    }

    /// Gives the terminal the saved modes back.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be changed.
    pub fn restore(&self) -> io::Result<()> {
        termios::tcsetattr(&self.terminal, OptionalActions::Now, &self.saved)
            .map_err(io::Error::from)
    }
}
