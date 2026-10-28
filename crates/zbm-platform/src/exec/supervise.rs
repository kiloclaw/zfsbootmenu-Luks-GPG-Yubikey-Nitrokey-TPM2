//! Commands with a time-out or a cancel token (ADR 0001, PM4): the thread
//! that runs one reads its standard output, and waits for it to end, in a
//! `poll` with the token and the deadline, and kills it when either comes
//! first. No thread watches the time; the child is killed through its pidfd,
//! before it is collected, so no other process that took its ID can be hit.

#![allow(
    clippy::disallowed_methods,
    reason = "the real runner times commands by the real clock, as the kernel kills them"
)]

use std::cell::Cell;
use std::io::{self, Read};
use std::os::fd::{AsFd, BorrowedFd};
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

use zbm_sys::{Process, Signal};

use super::Command;
use crate::event::CancelToken;
use crate::event::reactor::poll_readable;

/// How often a child is looked at where the kernel has no pidfd (before
/// Linux 5.3).
const POLL_WITHOUT_PIDFD: Duration = Duration::from_millis(10);

/// The limits of one running command, enforced by the thread that runs it.
pub(super) struct Supervisor {
    pid: u32,
    /// The child's pidfd, where the kernel has them.
    process: Option<Process>,
    deadline: Option<Instant>,
    cancel: Option<CancelToken>,
    /// Whether the child was killed.
    killed: Cell<bool>,
}

impl Supervisor {
    /// The limits of `command`, started as `child`; `None` when it has none.
    pub(super) fn new(command: &Command, child: &Child) -> Option<Supervisor> {
        if command.timeout.is_none() && command.cancel.is_none() {
            return None;
        }
        Some(Supervisor {
            pid: child.id(),
            process: Process::open(child.id()).ok(),
            deadline: command.timeout.map(|timeout| Instant::now() + timeout),
            cancel: command.cancel.clone(),
            killed: Cell::new(false),
        })
    }

    /// The time left until the deadline; `None` without one.
    fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    /// Whether the deadline passed or the token was cancelled.
    fn is_over(&self) -> bool {
        self.remaining() == Some(Duration::ZERO)
            || self.cancel.as_ref().is_some_and(CancelToken::is_cancelled)
    }

    /// Kills the child, once.
    fn kill(&self) {
        if self.killed.replace(true) {
            return;
        }
        let killed = self
            .process
            .as_ref()
            .map(|process| process.signal(Signal::Kill));
        if !matches!(killed, Some(Ok(()))) {
            // Not collected yet, so its ID is still its own.
            let _ = zbm_sys::send_signal(self.pid, Signal::Kill);
        }
    }

    /// Waits until `fd` is readable, or the deadline or the token comes
    /// first, which kills the child; whether to read `fd`.
    fn may_read(&self, fd: BorrowedFd<'_>) -> bool {
        loop {
            if self.killed.get() {
                return false;
            }
            if self.is_over() {
                self.kill();
                return false;
            }
            let mut fds = vec![fd];
            fds.extend(self.cancel.as_ref().map(AsFd::as_fd));
            match poll_readable(&fds, self.remaining()) {
                Ok(ready) if ready.first() == Some(&true) => return true,
                // Cancelled, timed out, or interrupted: looked at again.
                Ok(_) => {}
                Err(_) => return true,
            }
        }
    }

    /// Waits for the child to end, killing it at the deadline or when the
    /// token is cancelled, and collects it.
    ///
    /// # Errors
    ///
    /// When the child cannot be waited for.
    pub(super) fn wait(&self, child: &mut Child) -> io::Result<ExitStatus> {
        while !self.killed.get() {
            if self.is_over() {
                self.kill();
                break;
            }
            if let Some(process) = &self.process {
                let mut fds = vec![process.as_fd()];
                fds.extend(self.cancel.as_ref().map(AsFd::as_fd));
                if poll_readable(&fds, self.remaining())?.first() == Some(&true) {
                    break;
                }
            } else {
                if child.try_wait()?.is_some() {
                    break;
                }
                let pause = self
                    .remaining()
                    .map_or(POLL_WITHOUT_PIDFD, |left| left.min(POLL_WITHOUT_PIDFD));
                std::thread::sleep(pause);
            }
        }
        child.wait()
    }
}

/// A child's output, read only when there is something to read, which ends
/// when its [`Supervisor`] kills the child.
pub(super) struct Supervised<'a, R> {
    pub(super) reader: R,
    pub(super) supervisor: &'a Supervisor,
}

impl<R: Read + AsFd> Read for Supervised<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.supervisor.may_read(self.reader.as_fd()) {
            return Ok(0);
        }
        self.reader.read(buf)
    }
}
