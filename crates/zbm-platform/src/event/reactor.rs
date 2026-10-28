//! [`Reactor`]: the one `poll` of a process, over its sources of input and
//! the nearest of its deadlines.

use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec};

use crate::clock::Clock;

/// What woke the reactor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wake<K> {
    /// The source `K` is readable, or closed.
    Ready(K),
    /// The deadline `K` passed. It is removed; another can be set.
    Due(K),
}

/// The sources of input a reactor waits for, each named by a key `K`, and
/// its deadlines, at [`Clock::now`] times. [`Reactor::wait`] blocks in one
/// `poll` until a source is readable or the nearest deadline passes.
pub struct Reactor<'fd, K> {
    sources: Vec<(BorrowedFd<'fd>, K)>,
    deadlines: Vec<(Duration, K)>,
}

impl<K> Default for Reactor<'_, K> {
    fn default() -> Self {
        Reactor {
            sources: Vec::new(),
            deadlines: Vec::new(),
        }
    }
}

impl<'fd, K: Copy + PartialEq> Reactor<'fd, K> {
    /// A reactor that waits for nothing.
    pub fn new() -> Reactor<'fd, K> {
        Reactor::default()
    }

    /// Waits for `source` to be readable, as `key`.
    pub fn watch(&mut self, source: &'fd impl AsFd, key: K) {
        self.sources.push((source.as_fd(), key));
    }

    /// Stops waiting for the sources named `key`.
    pub fn unwatch(&mut self, key: K) {
        self.sources.retain(|(_, watched)| *watched != key);
    }

    /// Wakes at `at` (a [`Clock::now`] time) as `key`, replacing an earlier
    /// deadline of that key.
    pub fn at(&mut self, at: Duration, key: K) {
        self.forget(key);
        self.deadlines.push((at, key));
    }

    /// Drops the deadline `key`, if it is set.
    pub fn forget(&mut self, key: K) {
        self.deadlines.retain(|(_, named)| *named != key);
    }

    /// Blocks until a source is readable or the nearest deadline passes, as
    /// `clock` tells the time, then returns everything that woke it: the
    /// deadlines that passed first, then the ready sources. Without sources
    /// or deadlines it returns at once, empty; after a signal interrupted
    /// the `poll`, it may return empty too.
    ///
    /// # Errors
    ///
    /// When `poll` fails otherwise.
    pub fn wait(&mut self, clock: &dyn Clock) -> io::Result<Vec<Wake<K>>> {
        let mut woke = self.due(clock.now());
        if !woke.is_empty() || (self.sources.is_empty() && self.deadlines.is_empty()) {
            // What is due goes first; ready sources are taken without waiting.
            woke.extend(self.ready(Some(Duration::ZERO))?);
            return Ok(woke);
        }
        let now = clock.now();
        let timeout = self
            .deadlines
            .iter()
            .map(|(at, _)| at.saturating_sub(now))
            .min();
        let ready = self.ready(timeout)?;
        let mut woke = self.due(clock.now());
        woke.extend(ready);
        Ok(woke)
    }

    /// Removes and returns the deadlines at or before `now`, earliest first.
    fn due(&mut self, now: Duration) -> Vec<Wake<K>> {
        let mut due: Vec<(Duration, K)> = Vec::new();
        self.deadlines.retain(|deadline| {
            let passed = deadline.0 <= now;
            if passed {
                due.push(*deadline);
            }
            !passed
        });
        due.sort_by_key(|(at, _)| *at);
        due.into_iter().map(|(_, key)| Wake::Due(key)).collect()
    }

    /// The sources readable within `timeout`.
    fn ready(&self, timeout: Option<Duration>) -> io::Result<Vec<Wake<K>>> {
        let fds: Vec<BorrowedFd<'_>> = self.sources.iter().map(|(fd, _)| *fd).collect();
        let readable = poll_readable(&fds, timeout)?;
        Ok(self
            .sources
            .iter()
            .zip(readable)
            .filter(|(_, ready)| *ready)
            .map(|((_, key), _)| Wake::Ready(*key))
            .collect())
    }
}

/// Which of `fds` are readable (or closed) within `timeout` (`None`: no
/// limit); none after a signal interrupted the wait.
pub(crate) fn poll_readable(
    fds: &[BorrowedFd<'_>],
    timeout: Option<Duration>,
) -> io::Result<Vec<bool>> {
    let mut polled: Vec<PollFd<'_>> = fds
        .iter()
        .map(|fd| PollFd::new(fd, PollFlags::IN))
        .collect();
    let timespec = timeout.map(|timeout| Timespec {
        tv_sec: i64::try_from(timeout.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(timeout.subsec_nanos()),
    });
    match rustix::event::poll(&mut polled, timespec.as_ref()) {
        Ok(_) => {}
        Err(rustix::io::Errno::INTR) => return Ok(vec![false; fds.len()]),
        Err(error) => return Err(error.into()),
    }
    let woken = PollFlags::IN | PollFlags::HUP | PollFlags::ERR;
    Ok(polled
        .iter()
        .map(|fd| fd.revents().intersects(woken))
        .collect())
}

/// Whether `fd` is readable (or closed) within `timeout`.
#[cfg(test)]
pub(crate) fn readable(fd: &impl AsFd, timeout: Option<Duration>) -> io::Result<bool> {
    Ok(poll_readable(&[fd.as_fd()], timeout)?
        .first()
        .copied()
        .unwrap_or(false))
}

#[cfg(test)]
mod tests;
