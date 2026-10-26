//! The blinking of the cursor and of text (SGR 5 and 6), on timers that
//! run only while something on the screen blinks.

use std::time::{Duration, Instant};

/// How long what blinks is shown, and then hidden.
pub const BLINK: Duration = Duration::from_millis(500);

/// The blinking of the cursor: a timer that runs only while a blinking
/// cursor is shown.
#[derive(Debug, Default)]
pub struct Cursor {
    /// When the cursor is shown or hidden next.
    pub at: Option<Instant>,
    /// Whether it is hidden now.
    pub off: bool,
    /// Whether the programs wrote since the last tick: the cursor is shown
    /// again, as it is while they type.
    pub fed: bool,
}

impl Cursor {
    /// Starts, stops or turns the blinking at `now`: it runs only while
    /// `is_blinking`, and starts shown after output.
    pub fn time(&mut self, is_blinking: bool, now: Instant) {
        if !is_blinking {
            *self = Cursor::default();
        } else if std::mem::take(&mut self.fed) || self.at.is_none() {
            self.off = false;
            self.at = Some(now + BLINK);
        } else if self.at.is_some_and(|at| at <= now) {
            self.off = !self.off;
            self.at = Some(now + BLINK);
        }
    }
}

/// The blinking of text: a timer that runs only while the screen shows
/// blinking text.
#[derive(Debug, Default)]
pub struct Text {
    /// When the text is shown or hidden next.
    at: Option<Instant>,
    /// Whether it is hidden now.
    off: bool,
    /// When the console last ticked.
    last: Option<Instant>,
    /// Whether the text was shown or hidden since the last draw.
    turned: bool,
}

impl Text {
    /// Whether blinking text is hidden now.
    pub fn is_off(&self) -> bool {
        self.off
    }

    /// When the text is shown or hidden next, if it blinks.
    pub fn at(&self) -> Option<Instant> {
        self.at
    }

    /// Shows or hides the text at `now` when its time came.
    pub fn tick(&mut self, now: Instant) {
        self.last = Some(now);
        if self.at.is_some_and(|at| at <= now) {
            self.off = !self.off;
            self.at = Some(now + BLINK);
            self.turned = true;
        }
    }

    /// Whether the text was shown or hidden since the last call, which
    /// means it must be drawn again.
    pub fn has_turned(&mut self) -> bool {
        std::mem::take(&mut self.turned)
    }

    /// Starts the timer from the last tick when the screen drawn
    /// `has_blinking` text, or stops it, the text shown.
    pub fn follow(&mut self, has_blinking: bool) {
        if !has_blinking {
            self.at = None;
            self.off = false;
        } else if self.at.is_none() {
            self.at = self.last.map(|last| last + BLINK);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "the blinking is timed from a real instant"
)]
mod tests {
    use super::*;

    #[test]
    fn text_blinks_only_while_the_screen_shows_blinking_text() {
        let start = Instant::now();
        let mut text = Text::default();
        // Before any tick, there is no time to start from.
        text.follow(true);
        assert_eq!(text.at(), None);
        text.tick(start);
        text.follow(true);
        assert_eq!(text.at(), Some(start + BLINK));
        text.tick(start + BLINK / 2);
        assert!(!text.is_off() && !text.has_turned());
        text.tick(start + BLINK);
        assert!(text.is_off() && text.has_turned() && !text.has_turned());
        assert_eq!(text.at(), Some(start + 2 * BLINK));
        // The blinking text is gone: shown, and no timer.
        text.follow(false);
        assert!(!text.is_off());
        assert_eq!(text.at(), None);
    }
}
