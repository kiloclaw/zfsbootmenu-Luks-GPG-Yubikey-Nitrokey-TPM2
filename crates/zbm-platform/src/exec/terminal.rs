//! Programs on another terminal: a remote menu's (ROADMAP.md, PM10), whose
//! actions that take the console run in the session on the terminal the
//! menu handed it.

use std::io::{self, Write as _};
use std::os::fd::{AsFd as _, BorrowedFd, OwnedFd};
use std::process::Stdio;

use super::{Command, Output, Runner, run_on};

/// Runs commands as [`SystemRunner`](super::SystemRunner) does, the
/// streams they inherit on a terminal instead of this process's, each in
/// a session of its own without a controlling terminal: a program that
/// opens `/dev/tty` finds none, rather than this process's.
#[derive(Debug)]
pub struct TerminalRunner {
    input: OwnedFd,
    output: OwnedFd,
}

impl TerminalRunner {
    /// Runs commands on the terminal read on `input` and written on
    /// `output`.
    pub fn new(input: OwnedFd, output: OwnedFd) -> TerminalRunner {
        TerminalRunner { input, output }
    }
}

impl Runner for TerminalRunner {
    fn run(&self, command: &Command) -> io::Result<Output> {
        let inherited = Inherited {
            terminal: Some((self.input.as_fd(), self.output.as_fd())),
        };
        run_on(command, inherited)
    }
}

/// Where the streams a command inherits go: this process's, or a
/// terminal's ([`TerminalRunner`]).
#[derive(Clone, Copy, Default)]
pub(super) struct Inherited<'t> {
    /// The terminal's input and output, if a terminal's.
    pub(super) terminal: Option<(BorrowedFd<'t>, BorrowedFd<'t>)>,
}

impl Inherited<'_> {
    /// Standard input.
    pub(super) fn input(self) -> io::Result<Stdio> {
        match self.terminal {
            Some((input, _)) => Ok(Stdio::from(input.try_clone_to_owned()?)),
            None => Ok(Stdio::inherit()),
        }
    }

    /// Standard output or error.
    pub(super) fn output(self) -> io::Result<Stdio> {
        match self.terminal {
            Some((_, output)) => Ok(Stdio::from(output.try_clone_to_owned()?)),
            None => Ok(Stdio::inherit()),
        }
    }

    /// Standard error, for a copy of standard output (`2>&1`).
    pub(super) fn merged_error(self) -> io::Result<Stdio> {
        let output = match self.terminal {
            Some((_, output)) => output.try_clone_to_owned()?,
            None => std::os::fd::AsFd::as_fd(&io::stdout()).try_clone_to_owned()?,
        };
        Ok(Stdio::from(output))
    }

    /// Writes `message` where standard error goes.
    pub(super) fn write_error(self, message: &[u8]) -> io::Result<()> {
        match self.terminal {
            Some((_, output)) => {
                let mut output = std::fs::File::from(output.try_clone_to_owned()?);
                output.write_all(message)
            }
            None => io::stderr().write_all(message),
        }
    }
}
