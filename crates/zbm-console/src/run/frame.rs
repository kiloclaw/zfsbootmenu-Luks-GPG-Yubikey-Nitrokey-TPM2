//! Frames drawn and presented: at the display's refresh rate at most,
//! blanked when it is time, and given to the serial console too.

use std::io;
use std::time::{Duration, Instant};

use zbm_display::display::Screen;

use super::blank::Blanking;
use super::{Host, Shown, blank, show};
use crate::screen::Console;

/// Draws what changed, blanks the display when it is due, and presents
/// the frame when its time came at `now`; the serial console gets each
/// frame presented, and while the display is blanked each one drawn.
pub(super) fn draw_frame(
    (console, screen, host): (&mut Console, &mut dyn Screen, &mut dyn Host),
    (pacing, blanking): (&mut Pacing, &mut Blanking),
    shown: &mut Option<&mut dyn Shown>,
    now: Instant,
) -> io::Result<()> {
    let drawn = console.draw() > 0;
    pacing.waiting |= drawn;
    if blanking.is_due(now) {
        blank(console, screen)?;
        blanking.blanked();
    }
    if !blanking.is_blanked() {
        if pacing.present(console, screen, shown, Some(now))? {
            host.show_serial(console);
        }
    } else if drawn {
        host.show_serial(console);
    }
    Ok(())
}

/// Frames presented at the display's refresh rate at most (C75): when the
/// last was, and whether one drawn since waits for its time.
pub(super) struct Pacing {
    interval: Duration,
    presented: Instant,
    pub(super) waiting: bool,
}

impl Pacing {
    /// Frames `interval` apart; the first is presented at once.
    pub(super) fn new(interval: Duration) -> Pacing {
        let presented = Instant::now()
            .checked_sub(interval)
            .unwrap_or_else(Instant::now);
        Pacing {
            interval,
            presented,
            waiting: false,
        }
    }

    /// When the frame that waits is due, if one does.
    pub(super) fn due(&self) -> Option<Instant> {
        self.waiting.then(|| self.presented + self.interval)
    }

    /// Presents the frame that waits, if its time came at `now`, or at once
    /// without `now`, and shows its text; whether it presented one.
    pub(super) fn present(
        &mut self,
        console: &mut Console,
        screen: &mut dyn Screen,
        shown: &mut Option<&mut dyn Shown>,
        now: Option<Instant>,
    ) -> io::Result<bool> {
        let is_due = now.is_none_or(|now| now.duration_since(self.presented) >= self.interval);
        if !self.waiting || !is_due {
            return Ok(false);
        }
        screen.present(console.frame_mut())?;
        show(console, shown);
        self.presented = now.unwrap_or_else(Instant::now);
        self.waiting = false;
        Ok(true)
    }
}
