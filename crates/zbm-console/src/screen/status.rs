//! The status line (`zbm.console_status`): a last row under the programs'
//! screen with the title they set, the keyboard layout, the locks that are
//! on, the time and the battery's charge.

use std::time::{Duration, Instant};

use alacritty_terminal::term::test::TermSize;

use super::Console;

/// The locks of the keyboards.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Locks {
    /// Caps Lock.
    pub caps: bool,
    /// Num Lock.
    pub num: bool,
    /// Scroll Lock, which holds the programs' output.
    pub scroll: bool,
}

/// A time of day, in UTC: the boot image has no time zone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Clock {
    /// From 0 to 23.
    pub hour: u8,
    /// From 0 to 59.
    pub minute: u8,
    /// From 0 to 59.
    pub second: u8,
}

impl Clock {
    /// The time of day `seconds` after the Unix epoch.
    pub fn of(seconds: u64) -> Clock {
        let day = seconds % 86_400;
        let part = |value: u64| u8::try_from(value).unwrap_or(0);
        Clock {
            hour: part(day / 3600),
            minute: part(day / 60 % 60),
            second: part(day % 60),
        }
    }
}

/// What the status line says, from the host.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Status {
    /// The keyboard layout.
    pub layout: String,
    /// The locks that are on.
    pub locks: Locks,
    /// The time, when the host knows it.
    pub clock: Option<Clock>,
    /// The charge of the battery, in percent, on a laptop.
    pub battery: Option<u8>,
}

impl Status {
    /// The right of the line: the layout, the locks, the time, the battery.
    fn right(&self) -> String {
        let mut parts = vec![self.layout.clone()];
        for (is_on, name) in [
            (self.locks.caps, "Caps"),
            (self.locks.num, "Num"),
            (self.locks.scroll, "Scroll"),
        ] {
            if is_on {
                parts.push(name.to_owned());
            }
        }
        if let Some(clock) = self.clock {
            parts.push(format!("{:02}:{:02} UTC", clock.hour, clock.minute));
        }
        if let Some(battery) = self.battery {
            parts.push(format!("Battery {battery}%"));
        }
        parts.retain(|part| !part.is_empty());
        parts.join("  ")
    }

    /// The line of `columns` cells: `title` on the left, cut when it does
    /// not fit, and the rest on the right.
    pub fn line(&self, title: Option<&str>, columns: usize) -> Vec<char> {
        let right: Vec<char> = self.right().chars().collect();
        let room = columns.saturating_sub(right.len() + 3);
        let mut line = vec![' '];
        line.extend(title.unwrap_or_default().chars().take(room));
        let gap = columns.saturating_sub(line.len() + right.len() + 1);
        line.extend(std::iter::repeat_n(' ', gap));
        line.extend(right);
        line.push(' ');
        // When even the right does not fit, its end is kept.
        let extra = line.len().saturating_sub(columns);
        line.drain(..extra);
        line
    }
}

impl Console {
    /// The console with a status line under the programs' screen, which
    /// gets one row less, when it has two rows or more.
    #[must_use]
    pub fn with_status_line(mut self) -> Console {
        if self.status_line.is_none() && self.rows > 1 {
            self.status_line = Some(Vec::new());
            self.rows -= 1;
            let size = TermSize::new(usize::from(self.columns), usize::from(self.rows));
            self.term.resize(size);
            self.repaint();
        }
        self
    }

    /// Whether the console has a status line.
    pub fn has_status_line(&self) -> bool {
        self.status_line.is_some()
    }

    /// The rows of the display: the programs' screen's, and the status
    /// line's.
    pub(super) fn display_rows(&self) -> u16 {
        self.rows + u16::from(self.has_status_line())
    }

    /// Says `status` on the status line, if there is one, at `now`; the
    /// console wakes for the next minute when it has the time.
    pub fn set_status(&mut self, status: &Status, now: Instant) {
        if self.status_line.is_none() {
            return;
        }
        // The line shows the time to the minute.
        let shown = Status {
            clock: status.clock.map(|clock| Clock { second: 0, ..clock }),
            ..status.clone()
        };
        let said = (shown, self.title.clone(), self.columns);
        if self.status_said.as_ref() != Some(&said) {
            let line = status.line(self.title.as_deref(), usize::from(self.columns));
            self.status_line = Some(line);
            self.status_said = Some(said);
        }
        self.status_at = status
            .clock
            .map(|clock| now + Duration::from_secs(60 - u64::from(clock.second.min(59))));
    }

    /// When the status line must be said again, for the time.
    pub(super) fn status_at(&self) -> Option<Instant> {
        self.status_at
    }

    /// The status line as it is said.
    pub(super) fn status_text(&self) -> Option<&[char]> {
        self.status_line.as_deref()
    }
}
