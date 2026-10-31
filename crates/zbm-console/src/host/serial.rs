//! The serial console the console serves too (ROADMAP.md UI9): its device
//! open without becoming the console's controlling terminal, in raw mode
//! while the console has it and as it was afterwards, the keys read from
//! it and the screen written to it ([`crate::serial`]).

use std::fs::{File, OpenOptions};
use std::io::{self, Read as _};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::time::{Duration, Instant};

use rustix::termios::{self, OptionalActions, Termios};
use zbm_display::keys::KeyPress;

use super::{O_NOCTTY, RealHost};
use crate::scheme::Scheme;
use crate::screen::Console;
use crate::serial::{SerialInput, SerialView, has_repaint_key};

/// How much is read from the serial line at once.
const READ: usize = 256;

/// A serial terminal: its device, what is typed there, and its screen.
pub struct SerialPort {
    file: File,
    input: SerialInput,
    view: SerialView<File>,
    /// The terminal's modes before the console made it raw.
    saved: Option<Termios>,
    /// Whether the line went away: it is not read any more, and its
    /// screen still written as it takes it.
    is_gone: bool,
}

impl std::fmt::Debug for SerialPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SerialPort")
            .field("size", &self.view.size())
            .finish_non_exhaustive()
    }
}

impl SerialPort {
    /// Opens the serial terminal at `path`, of `size` (columns, rows), its
    /// screen in the colours of `scheme`.
    ///
    /// # Errors
    ///
    /// When it cannot be opened or written without blocking.
    #[allow(
        clippy::disallowed_methods,
        reason = "the serial line is this host's own device, as the terminal is"
    )]
    pub fn open(path: &Path, size: (u16, u16), scheme: &'static Scheme) -> io::Result<SerialPort> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(O_NOCTTY)
            .open(path)?;
        rustix::io::ioctl_fionbio(&file, true)?;
        let saved = termios::tcgetattr(&file).ok();
        if let Some(saved) = &saved {
            let mut raw = saved.clone();
            raw.make_raw();
            // Lines end as written, and the kernel's own messages on the
            // console still start their lines.
            raw.output_modes |= termios::OutputModes::OPOST | termios::OutputModes::ONLCR;
            // A line without modem signals does not hang up.
            raw.control_modes |= termios::ControlModes::CLOCAL | termios::ControlModes::CREAD;
            let _ = termios::tcsetattr(&file, OptionalActions::Now, &raw);
        }
        let view = SerialView::new(file.try_clone()?, size, scheme);
        Ok(SerialPort {
            file,
            input: SerialInput::default(),
            view,
            saved,
            is_gone: false,
        })
    }

    /// The device, to wait on.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }

    /// Whether the line is read: until it went away.
    pub fn is_read(&self) -> bool {
        !self.is_gone
    }

    /// The terminal's size: columns and rows.
    pub fn size(&self) -> (u16, u16) {
        self.view.size()
    }

    /// The keys typed since, read as far as the line has them, at `now`;
    /// Ctrl+L also has the whole screen written again. A line that hung up
    /// or fails is read no more.
    pub fn read_keys(&mut self, now: Instant) -> Vec<KeyPress> {
        let mut keys = Vec::new();
        let mut buffer = [0_u8; READ];
        loop {
            match self.file.read(&mut buffer) {
                Ok(length) if length > 0 => {
                    keys.extend(
                        self.input
                            .take(buffer.get(..length).unwrap_or_default(), now),
                    );
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Ok(_) | Err(_) => {
                    self.is_gone = true;
                    break;
                }
            }
        }
        zbm_sys::wipe(&mut buffer);
        if has_repaint_key(&keys) {
            self.view.repaint();
        }
        keys
    }

    /// The keys of a sequence that waited for its rest, once its time ran
    /// out at `now`.
    pub fn expire(&mut self, now: Instant) -> Vec<KeyPress> {
        self.input.expire(now)
    }

    /// How long the host may wait at most: until a sequence that waits is
    /// taken as it is, or a little while the screen waits to be written.
    pub fn idle(&self, now: Instant, idle: Duration) -> Duration {
        let waiting = self
            .input
            .deadline()
            .map(|deadline| deadline.saturating_duration_since(now));
        let pending = self.view.is_pending().then_some(crate::run::MIRROR_RETRY);
        [Some(idle), waiting, pending]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(idle)
    }

    /// Whether the screen waits to be written.
    pub fn is_pending(&self) -> bool {
        self.view.is_pending()
    }

    /// Takes the screen of `console`, and writes what the line takes.
    pub fn show(&mut self, console: &Console) {
        self.view.show(&console.styled());
    }

    /// Writes what the line takes now.
    pub fn flush(&mut self) {
        self.view.flush();
    }
}

impl Drop for SerialPort {
    /// The terminal gets its modes back, for the kernel's console.
    fn drop(&mut self) {
        if let Some(saved) = &self.saved {
            let _ = termios::tcsetattr(&self.file, OptionalActions::Drain, saved);
        }
    }
}

impl<M> RealHost<'_, M> {
    /// The host, which serves the serial terminal `port` too.
    #[must_use]
    pub fn with_serial(mut self, port: SerialPort) -> Self {
        self.serial = Some(port);
        self
    }

    /// The keys of the serial terminal, when its readiness `ready` says it
    /// has some, or a sequence that waited ran out of time; its screen
    /// written further when it has room.
    pub(super) fn serial_keys(
        &mut self,
        ready: Option<zbm_sys::console::Ready>,
    ) -> Option<Vec<KeyPress>> {
        let port = self.serial.as_mut()?;
        let now = Instant::now();
        let mut keys = port.expire(now);
        if let Some(ready) = ready {
            if ready.write {
                port.flush();
            }
            if (ready.read || ready.closed) && port.is_read() {
                keys.extend(port.read_keys(now));
            }
        }
        (!keys.is_empty()).then_some(keys)
    }
}

#[cfg(test)]
mod tests;
