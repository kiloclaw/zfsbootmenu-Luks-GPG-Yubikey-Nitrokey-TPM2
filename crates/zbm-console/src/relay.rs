//! The relay that carries the programs' terminal over to the kernel
//! console once the display is handed back (`zbm console release`, before
//! `kexec`): what they write is shown there, and what is typed there
//! reaches them, until they are gone or the display is taken again.

#![allow(
    clippy::disallowed_methods,
    reason = "the pseudo-terminal and the kernel console are this module's system interface"
)]

use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd as _, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rustix::termios::{self, OptionalActions};
use zbm_sys::console::{Watch, wait_ready};

use crate::host::{CHUNK, Outgoing, read_output};
use crate::run::Input;
use crate::screen::Console;

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
/// is set: what they write is shown there, and fed to `kept`, the
/// console's terminal kept for when the display is taken again; what is
/// typed there reaches them. The programs get the console's size.
///
/// # Errors
///
/// When one side cannot be read or written.
pub fn relay<M, T>(
    master: &mut M,
    terminal: &mut T,
    reattach: Option<&AtomicBool>,
    kept: Option<&mut Console>,
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
        kept,
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

/// The two sides of the relay, what was typed for the programs, and the
/// console's terminal, kept.
struct Sides<'s, M, T> {
    master: &'s mut M,
    terminal: &'s mut T,
    outgoing: Outgoing,
    typed: Vec<u8>,
    kept: Option<&'s mut Console>,
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
            Watch::read(self.terminal.as_fd().as_raw_fd()),
            Watch {
                fd: self.master.as_fd().as_raw_fd(),
                read: true,
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
                Input::Output(mut bytes) => {
                    let written = self.terminal.write_all(&bytes);
                    if let Some(console) = self.kept.as_deref_mut() {
                        // The kernel console answers the programs now.
                        console.feed(&bytes);
                        zbm_sys::wipe_vec(&mut console.take_replies());
                    }
                    zbm_sys::wipe_vec(&mut bytes);
                    written?;
                }
                Input::Hangup => return Ok(Some(Relayed::Hangup)),
                _ => {}
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
