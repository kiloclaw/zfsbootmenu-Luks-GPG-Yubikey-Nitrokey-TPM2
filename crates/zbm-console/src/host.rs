//! The real host of the console's loop: the controlling side of the
//! pseudo-terminal, the keyboards, and the request to hand the display
//! back; and the relay that carries the terminal over to the kernel
//! console afterwards.

#![allow(
    clippy::disallowed_methods,
    reason = "the pseudo-terminal and the kernel console are this module's system interface"
)]

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd as _, OwnedFd};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use alacritty_terminal::event::WindowSize;
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{self, OptionalActions, Winsize};
use zbm_display::input::Keyboards;
use zbm_sys::console::first_readable;

use crate::run::{Host, Input};

/// How much is read at once.
const CHUNK: usize = 16 * 1024;

/// Reads what the programs wrote on `master`; at end of file, or `EIO`,
/// which Linux reports once no program holds the other side, they are
/// gone.
///
/// # Errors
///
/// When `master` cannot be read otherwise.
pub fn read_output(master: &mut impl Read) -> io::Result<Input> {
    let mut buffer = vec![0_u8; CHUNK];
    match master.read(&mut buffer) {
        Ok(0) => Ok(Input::Hangup),
        Ok(length) => {
            buffer.truncate(length);
            Ok(Input::Output(buffer))
        }
        Err(error) if error.raw_os_error() == Some(rustix::io::Errno::IO.raw_os_error()) => {
            Ok(Input::Hangup)
        }
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(Input::Nothing),
        Err(error) => Err(error),
    }
}

/// Opens a pseudo-terminal: its controlling side, for the console, and the
/// programs' side. Neither becomes this process's controlling terminal,
/// and neither is inherited by programs started with other files.
///
/// # Errors
///
/// When the kernel has no pseudo-terminals to give.
pub fn open_pty() -> io::Result<(File, File)> {
    let flags = OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC;
    let master = openpt(flags)?;
    grantpt(&master)?;
    unlockpt(&master)?;
    let name = ptsname(&master, Vec::new())?;
    let slave = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(O_NOCTTY)
        .open(OsStr::from_bytes(name.as_bytes()))?;
    Ok((File::from(master), slave))
}

/// `O_NOCTTY`, the same on every Linux architecture.
const O_NOCTTY: i32 = 0o400;

/// Tells the programs on `master` the size of their terminal.
///
/// # Errors
///
/// When `master` is not a terminal.
pub fn set_size(master: impl AsFd, size: &WindowSize) -> io::Result<()> {
    let winsize = Winsize {
        ws_row: size.num_lines,
        ws_col: size.num_cols,
        ws_xpixel: size.num_cols.saturating_mul(size.cell_width),
        ws_ypixel: size.num_lines.saturating_mul(size.cell_height),
    };
    termios::tcsetwinsize(master, winsize).map_err(io::Error::from)
}

/// The pseudo-terminal, the keyboards and the request to hand the display
/// back.
#[derive(Debug)]
pub struct RealHost<M> {
    master: M,
    keyboards: Keyboards,
    release: Arc<AtomicBool>,
}

impl<M: Read + Write + AsFd> RealHost<M> {
    /// A host on `master`, with keys from `keyboards`; `release` is set
    /// when the display must be handed back.
    pub fn new(master: M, keyboards: Keyboards, release: Arc<AtomicBool>) -> RealHost<M> {
        RealHost {
            master,
            keyboards,
            release,
        }
    }

    /// The controlling side of the pseudo-terminal, given back.
    pub fn into_master(self) -> M {
        self.master
    }
}

impl<M: Read + Write + AsFd> Host for RealHost<M> {
    fn wait(&mut self, timeout: Duration) -> io::Result<Input> {
        if self.release.load(Ordering::SeqCst) {
            return Ok(Input::Release);
        }
        let mut fds = vec![self.master.as_fd().as_raw_fd()];
        fds.extend(self.keyboards.fds());
        match first_readable(&fds, Some(timeout))? {
            None if self.release.load(Ordering::SeqCst) => Ok(Input::Release),
            None => Ok(Input::Nothing),
            Some(0) => read_output(&mut self.master),
            Some(index) => Ok(Input::Keys(self.keyboards.read_device(index - 1)?)),
        }
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.master.write_all(bytes)
    }
}

/// The kernel console in raw mode, so that each key reaches the programs
/// as typed and the pseudo-terminal does the echoing; restored when
/// dropped. Nothing is changed on what is not a terminal.
struct Raw {
    terminal: Option<OwnedFd>,
    saved: Option<termios::Termios>,
}

impl Raw {
    fn new(terminal: impl AsFd) -> Raw {
        let saved = termios::tcgetattr(&terminal).ok();
        if let Some(saved) = &saved {
            let mut raw = saved.clone();
            raw.make_raw();
            let _ = termios::tcsetattr(&terminal, OptionalActions::Now, &raw);
        }
        Raw {
            terminal: terminal.as_fd().try_clone_to_owned().ok(),
            saved,
        }
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        if let (Some(terminal), Some(saved)) = (&self.terminal, &self.saved) {
            let _ = termios::tcsetattr(terminal, OptionalActions::Now, saved);
        }
    }
}

/// Relays between the programs on `master` and `terminal`, the kernel
/// console, until the programs are gone: what they write is shown there,
/// what is typed there reaches them. The programs get the console's size.
///
/// # Errors
///
/// When one side cannot be read or written.
pub fn relay<M, T>(master: &mut M, terminal: &mut T) -> io::Result<()>
where
    M: Read + Write + AsFd,
    T: Read + Write + AsFd,
{
    if let Ok(size) = termios::tcgetwinsize(&*terminal) {
        let _ = termios::tcsetwinsize(&*master, size);
    }
    let raw = Raw::new(&*terminal);
    let fds = [master.as_fd().as_raw_fd(), terminal.as_fd().as_raw_fd()];
    let mut typed = vec![0_u8; CHUNK];
    let outcome = loop {
        match first_readable(&fds, None) {
            Ok(Some(0)) => match read_output(master) {
                Ok(Input::Output(bytes)) => {
                    if let Err(error) = terminal.write_all(&bytes) {
                        break Err(error);
                    }
                }
                Ok(Input::Hangup) => break Ok(()),
                Ok(_) => {}
                Err(error) => break Err(error),
            },
            Ok(Some(_)) => match terminal.read(&mut typed) {
                Ok(0) => break Ok(()),
                Ok(length) => {
                    let sent = master.write_all(typed.get(..length).unwrap_or(&[]));
                    zbm_sys::wipe(&mut typed);
                    if let Err(error) = sent {
                        break Err(error);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => break Err(error),
            },
            Ok(None) => {}
            Err(error) => break Err(error),
        }
    };
    drop(raw);
    outcome
}

#[cfg(test)]
mod tests;
