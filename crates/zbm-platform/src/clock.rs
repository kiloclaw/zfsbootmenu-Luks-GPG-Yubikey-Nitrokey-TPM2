//! Time.
//!
//! Countdowns, retry delays and waiting for devices read and pass time through
//! [`Clock`], so tests run them instantly with a fake clock that only moves
//! when the code sleeps.

#![allow(
    clippy::disallowed_methods,
    reason = "SystemClock is where the time is read and waited for"
)]

use std::time::{Duration, Instant, SystemTime};

/// The time, and waiting.
pub trait Clock: Send + Sync {
    /// Time since an arbitrary start; it never goes backwards.
    fn now(&self) -> Duration;

    /// The wall-clock time, for time stamps.
    fn wall(&self) -> SystemTime;

    /// Waits for `duration`.
    fn sleep(&self, duration: Duration);
}

/// The system clock.
#[derive(Clone, Copy, Debug)]
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    /// A clock whose [`Clock::now`] starts at zero.
    pub fn new() -> SystemClock {
        SystemClock {
            start: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> SystemClock {
        SystemClock::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    fn wall(&self) -> SystemTime {
        SystemTime::now()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_clock_moves_forward() {
        let clock = SystemClock::default();
        let before = clock.now();
        clock.sleep(Duration::from_millis(20));
        assert!(clock.now() >= before + Duration::from_millis(20));
        assert!(clock.wall() > SystemTime::UNIX_EPOCH);
    }
}
