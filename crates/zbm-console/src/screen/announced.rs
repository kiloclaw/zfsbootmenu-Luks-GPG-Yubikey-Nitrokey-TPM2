//! The lines the menu says for a screen reader (`Announce`, ROADMAP.md
//! UI8, ADR 0011): the latest is shown for a while on the status line,
//! when the console has one, and over the last row of the text it writes
//! for `brltty`.

use std::time::{Duration, Instant};

use super::Console;

/// How long a line is said.
pub const ANNOUNCED: Duration = Duration::from_secs(5);

impl Console {
    /// Says `text` from `now` for five seconds (`ANNOUNCED`); what does not
    /// print is left out.
    pub fn announce(&mut self, text: &str, now: Instant) {
        let said: String = text.chars().filter(|c| !c.is_control()).collect();
        self.announced = Some((said, now + ANNOUNCED));
        // The status line says it at once.
        self.status_said = None;
    }

    /// What is said at `now`, if anything.
    pub fn announcement(&self, now: Instant) -> Option<&str> {
        self.announced
            .as_ref()
            .filter(|(_, until)| now < *until)
            .map(|(text, _)| text.as_str())
    }

    /// Until when the line said is said, for the status line.
    pub(super) fn announced_until(&self) -> Option<Instant> {
        self.announced.as_ref().map(|(_, until)| *until)
    }
}

#[cfg(test)]
mod tests;
