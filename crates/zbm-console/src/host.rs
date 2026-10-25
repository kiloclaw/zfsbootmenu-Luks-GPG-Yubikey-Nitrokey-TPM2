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
use zbm_sys::console::{Ready, Watch, wait_ready};

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
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) =>
        {
            Ok(Input::Nothing)
        }
        Err(error) => Err(error),
    }
}

/// How much typed text waits for the programs, at most: what is typed
/// beyond it while they read nothing is lost, as the kernel's terminal
/// loses it.
const OUTGOING: usize = 64 * 1024;

/// What was typed for the programs and not yet taken by the terminal. The
/// controlling side of the pseudo-terminal does not block, so that a
/// program that reads nothing cannot stop the console; the bytes wait here
/// and are wiped: they may be a passphrase.
#[derive(Debug)]
pub struct Outgoing {
    bytes: Vec<u8>,
}

impl Default for Outgoing {
    fn default() -> Outgoing {
        Outgoing {
            bytes: Vec::with_capacity(OUTGOING),
        }
    }
}

impl Drop for Outgoing {
    fn drop(&mut self) {
        zbm_sys::wipe_vec(&mut self.bytes);
    }
}

impl Outgoing {
    /// Whether nothing waits.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Queues `bytes`, as much as there is room for.
    pub fn push(&mut self, bytes: &[u8]) {
        let room = OUTGOING.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(bytes.get(..room.min(bytes.len())).unwrap_or(&[]));
    }

    /// Writes what waits to `master`, as much as it takes now.
    ///
    /// # Errors
    ///
    /// When `master` cannot be written other than for want of room.
    pub fn flush(&mut self, master: &mut impl Write) -> io::Result<()> {
        let mut written = 0;
        let outcome = loop {
            let Some(rest) = self.bytes.get(written..).filter(|rest| !rest.is_empty()) else {
                break Ok(());
            };
            match master.write(rest) {
                Ok(0) => break Ok(()),
                Ok(length) => written += length,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        if written > 0 {
            self.bytes.drain(..written);
            // Draining moved the rest down; the old bytes behind it are
            // overwritten.
            for slot in self.bytes.spare_capacity_mut() {
                slot.write(0);
            }
        }
        outcome
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

/// What the console does when asked for another keyboard layout: takes
/// the request and gives the keyboards the layout.
pub type Switch<'h> = Box<dyn FnMut(&mut Keyboards) + 'h>;

/// The pseudo-terminal, the keyboards, the request to hand the display
/// back and the request for another keyboard layout.
pub struct RealHost<'h, M> {
    master: M,
    outgoing: Outgoing,
    keyboards: Keyboards,
    release: Arc<AtomicBool>,
    layout: Option<(Arc<AtomicBool>, Switch<'h>)>,
}

impl<M: std::fmt::Debug> std::fmt::Debug for RealHost<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealHost")
            .field("master", &self.master)
            .field("keyboards", &self.keyboards)
            .finish_non_exhaustive()
    }
}

impl<'h, M: Read + Write + AsFd> RealHost<'h, M> {
    /// A host on `master`, with keys from `keyboards`; `release` is set
    /// when the display must be handed back. `master` stops blocking.
    pub fn new(master: M, keyboards: Keyboards, release: Arc<AtomicBool>) -> RealHost<'h, M> {
        let _ = rustix::io::ioctl_fionbio(&master, true);
        RealHost {
            master,
            outgoing: Outgoing::default(),
            keyboards,
            release,
            layout: None,
        }
    }

    /// The host, which runs `switch` when `requested` is set: a keyboard
    /// layout was asked for.
    #[must_use]
    pub fn with_layouts(mut self, requested: Arc<AtomicBool>, switch: Switch<'h>) -> Self {
        self.layout = Some((requested, switch));
        self
    }

    /// The keyboard layout keys are translated with.
    pub fn keyboard_layout(&self) -> &str {
        self.keyboards.layout()
    }

    /// The controlling side of the pseudo-terminal, given back; what was
    /// typed and not taken is dropped.
    pub fn into_master(self) -> M {
        self.master
    }

    /// Keys from the first keyboard that has some: the keyboards come
    /// before the programs' output, so that Ctrl-C reaches a program that
    /// floods the terminal.
    fn keys(&mut self, ready: &[Ready]) -> io::Result<Option<Input>> {
        let Some(index) = ready.iter().position(|ready| ready.is_ready()) else {
            return Ok(None);
        };
        let presses = self.keyboards.read_device(index)?;
        Ok(Some(if presses.is_empty() {
            Input::Nothing
        } else {
            Input::Keys(presses)
        }))
    }

    /// Gives the keyboards the layout asked for, if one was.
    fn switch_layout(&mut self) {
        if let Some((requested, switch)) = &mut self.layout
            && requested.swap(false, Ordering::SeqCst)
        {
            switch(&mut self.keyboards);
        }
    }
}

impl<M: Read + Write + AsFd> Host for RealHost<'_, M> {
    fn wait(&mut self, timeout: Duration) -> io::Result<Input> {
        if self.release.load(Ordering::SeqCst) {
            return Ok(Input::Release);
        }
        self.switch_layout();
        let mut watches: Vec<Watch> = self
            .keyboards
            .fds()
            .into_iter()
            .map(|fd| Watch { fd, write: false })
            .collect();
        watches.push(Watch {
            fd: self.master.as_fd().as_raw_fd(),
            write: !self.outgoing.is_empty(),
        });
        let ready = wait_ready(&watches, Some(timeout))?;
        let (master, keyboards) = match ready.split_last() {
            Some((master, keyboards)) => (*master, keyboards),
            None => (Ready::default(), &[][..]),
        };
        if master.write {
            self.outgoing.flush(&mut self.master)?;
        }
        if let Some(keys) = self.keys(keyboards)? {
            return Ok(keys);
        }
        if master.read || master.closed {
            return read_output(&mut self.master);
        }
        if self.release.load(Ordering::SeqCst) {
            return Ok(Input::Release);
        }
        if !master.write {
            // Nothing happened for a while: a keyboard may have come.
            self.keyboards.rescan();
            self.switch_layout();
        }
        Ok(Input::Nothing)
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.outgoing.push(bytes);
        self.outgoing.flush(&mut self.master)
    }
}

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

/// Why the relay on the kernel console ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Relayed {
    /// The programs are gone, or the kernel console closed.
    Hangup,
    /// The display was asked for again: `kexec` failed after the hand-over.
    Reattach,
}

/// Relays between the programs on `master` and `terminal`, the kernel
/// console, until the programs are gone, or until `reattach`, when given,
/// is set: what they write is shown there, what is typed there reaches
/// them. The programs get the console's size.
///
/// # Errors
///
/// When one side cannot be read or written.
pub fn relay<M, T>(
    master: &mut M,
    terminal: &mut T,
    reattach: Option<&AtomicBool>,
) -> io::Result<Relayed>
where
    M: Read + Write + AsFd,
    T: Read + Write + AsFd,
{
    if let Ok(size) = termios::tcgetwinsize(&*terminal) {
        let _ = termios::tcsetwinsize(&*master, size);
    }
    let _ = rustix::io::ioctl_fionbio(&*master, true);
    let raw = Raw::new(&*terminal);
    let mut sides = Sides {
        master,
        terminal,
        outgoing: Outgoing::default(),
        typed: vec![0_u8; CHUNK],
    };
    // With a request to wait for, the relay looks for it every tick.
    let timeout = reattach.map(|_| crate::run::TICK);
    let outcome = loop {
        if reattach.is_some_and(|asked| asked.swap(false, Ordering::SeqCst)) {
            break Ok(Relayed::Reattach);
        }
        match sides.step(timeout) {
            Ok(None) => {}
            Ok(Some(relayed)) => break Ok(relayed),
            Err(error) => break Err(error),
        }
    };
    drop(raw);
    outcome
}

/// The two sides of the relay, and what was typed for the programs.
struct Sides<'s, M, T> {
    master: &'s mut M,
    terminal: &'s mut T,
    outgoing: Outgoing,
    typed: Vec<u8>,
}

impl<M, T> Sides<'_, M, T>
where
    M: Read + Write + AsFd,
    T: Read + Write + AsFd,
{
    /// Waits at most `timeout` (`None`: without limit) and carries what
    /// one side has to the other; `Some` when the relay ends.
    fn step(&mut self, timeout: Option<Duration>) -> io::Result<Option<Relayed>> {
        // What is typed comes first, so that Ctrl-C reaches a program that
        // floods the terminal.
        let watches = [
            Watch {
                fd: self.terminal.as_fd().as_raw_fd(),
                write: false,
            },
            Watch {
                fd: self.master.as_fd().as_raw_fd(),
                write: !self.outgoing.is_empty(),
            },
        ];
        let ready = wait_ready(&watches, timeout)?;
        let (from_user, from_programs) = (
            ready.first().copied().unwrap_or_default(),
            ready.get(1).copied().unwrap_or_default(),
        );
        if from_programs.write {
            self.outgoing.flush(self.master)?;
        }
        if from_user.is_ready() {
            match self.terminal.read(&mut self.typed) {
                Ok(0) => return Ok(Some(Relayed::Hangup)),
                Ok(length) => {
                    self.outgoing.push(self.typed.get(..length).unwrap_or(&[]));
                    zbm_sys::wipe(&mut self.typed);
                    self.outgoing.flush(self.master)?;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        } else if from_programs.read || from_programs.closed {
            match read_output(self.master)? {
                Input::Output(bytes) => self.terminal.write_all(&bytes)?,
                Input::Hangup => return Ok(Some(Relayed::Hangup)),
                _ => {}
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
