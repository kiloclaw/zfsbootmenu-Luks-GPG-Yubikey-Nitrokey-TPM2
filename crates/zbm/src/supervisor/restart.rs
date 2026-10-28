//! When the supervisor starts a service again once it ended.
//!
//! The start-up program starts again with `--resume` once the recovery
//! shell ends; the shell starts when it is needed, with its fallback when
//! it does not open, and again after a while when it cannot start; the
//! console on the display starts again after a growing delay, a few times
//! at most ([`Backoff`]), and the programs stay on the kernel console after
//! that.

use std::time::Duration;

/// Restarts with a growing delay: `first`, then twice that each time, up to
/// `max`, at most `tries` times in a row. A run of at least `healthy`
/// starts the count again, so that a service that ends now and then is not
/// given up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Backoff {
    /// How many times in a row the service is started again, at most.
    pub tries: u32,
    /// The delay before the first restart.
    pub first: Duration,
    /// The longest delay.
    pub max: Duration,
    /// How long a run must last to count as healthy.
    pub healthy: Duration,
}

impl Backoff {
    /// No restart.
    pub const NEVER: Backoff = Backoff {
        tries: 0,
        first: Duration::ZERO,
        max: Duration::ZERO,
        healthy: Duration::ZERO,
    };

    /// The console on the display's: three tries, after 1, 2 and 4
    /// seconds; a minute of running starts the count again.
    pub const CONSOLE: Backoff = Backoff {
        tries: 3,
        first: Duration::from_secs(1),
        max: Duration::from_secs(8),
        healthy: Duration::from_secs(60),
    };

    /// The delay before restart `attempt` (0 for the first), or `None`
    /// once the tries are used up.
    pub fn delay(&self, attempt: u32) -> Option<Duration> {
        if attempt >= self.tries {
            return None;
        }
        let factor = 1_u32.checked_shl(attempt).unwrap_or(u32::MAX);
        Some(self.first.saturating_mul(factor).min(self.max))
    }

    /// The attempt that comes after a run of `ran` that ended, when the
    /// last one was `attempt`: the count starts again after a healthy run.
    pub fn after_run(&self, attempt: u32, ran: Duration) -> u32 {
        if ran >= self.healthy { 0 } else { attempt }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_double_up_to_the_most_and_end_after_the_tries() {
        let delays: Vec<Option<Duration>> = (0..5)
            .map(|attempt| Backoff::CONSOLE.delay(attempt))
            .collect();
        assert_eq!(
            delays,
            [
                Some(Duration::from_secs(1)),
                Some(Duration::from_secs(2)),
                Some(Duration::from_secs(4)),
                None,
                None
            ]
        );
        let long = Backoff {
            tries: 40,
            ..Backoff::CONSOLE
        };
        assert_eq!(long.delay(39), Some(Duration::from_secs(8)));
        assert_eq!(Backoff::NEVER.delay(0), None);
    }

    #[test]
    fn a_healthy_run_starts_the_count_again() {
        let backoff = Backoff::CONSOLE;
        assert_eq!(backoff.after_run(2, Duration::from_secs(5)), 2);
        assert_eq!(backoff.after_run(2, Duration::from_secs(60)), 0);
    }
}
