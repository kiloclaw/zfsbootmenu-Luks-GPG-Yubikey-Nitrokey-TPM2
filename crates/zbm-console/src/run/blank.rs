//! Blanking (C64, `zbm.console_blank=<minutes>`): after that long without
//! a key, the screen goes black and the display's power off, as `setterm
//! -blank` and `-powerdown` do; a key turns it on and shows the screen
//! again, and reaches the programs as any other.

use std::time::{Duration, Instant};

/// Whether the screen is blanked, and when it will be.
#[derive(Clone, Copy, Debug)]
pub(super) struct Blanking {
    /// How long without a key blanks it; never with `None`.
    after: Option<Duration>,
    /// The last key, or the start.
    active: Instant,
    is_blanked: bool,
}

impl Blanking {
    /// Blanking after `after` without a key, counted from `now`.
    pub(super) fn new(after: Option<Duration>, now: Instant) -> Blanking {
        Blanking {
            after,
            active: now,
            is_blanked: false,
        }
    }

    /// When the screen blanks, unless a key comes first.
    pub(super) fn due(&self) -> Option<Instant> {
        (!self.is_blanked)
            .then(|| self.after.map(|after| self.active + after))
            .flatten()
    }

    /// Whether the time to blank came at `now`.
    pub(super) fn is_due(&self, now: Instant) -> bool {
        self.due().is_some_and(|due| due <= now)
    }

    /// The screen was blanked.
    pub(super) fn blanked(&mut self) {
        self.is_blanked = true;
    }

    /// Whether the screen is blanked.
    pub(super) fn is_blanked(&self) -> bool {
        self.is_blanked
    }

    /// A key came at `now`; whether it wakes the screen.
    pub(super) fn is_woken_by_key(&mut self, now: Instant) -> bool {
        self.active = now;
        std::mem::take(&mut self.is_blanked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_screen_blanks_after_a_while_without_a_key_and_a_key_wakes_it() {
        let start = Instant::now();
        let minute = Duration::from_secs(60);
        let mut blanking = Blanking::new(Some(minute), start);

        assert_eq!(blanking.due(), Some(start + minute));
        assert!(!blanking.is_due(start + minute / 2));
        // A key counts the time again.
        assert!(!blanking.is_woken_by_key(start + minute / 2));
        assert!(!blanking.is_due(start + minute));
        assert!(blanking.is_due(start + minute * 2));
        blanking.blanked();
        assert!(blanking.is_blanked());
        assert_eq!(blanking.due(), None);
        assert!(blanking.is_woken_by_key(start + minute * 3));
        assert!(!blanking.is_blanked());

        let never = Blanking::new(None, start);
        assert_eq!(never.due(), None);
        assert!(!never.is_due(start + minute * 1000));
    }
}
