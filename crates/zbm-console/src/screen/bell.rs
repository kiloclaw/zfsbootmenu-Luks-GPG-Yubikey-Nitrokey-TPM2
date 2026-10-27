//! The bell (BEL): the frame's border flashes, the PC speaker sounds,
//! both, or neither (`zbm.console_bell`).

use std::time::{Duration, Instant};

use zbm_display::frame::Area;

use super::Console;

/// How long the border flashes.
pub const FLASH: Duration = Duration::from_millis(150);

/// What the bell does.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Ring {
    /// Nothing.
    Off,
    /// The frame's border flashes.
    #[default]
    Visual,
    /// The PC speaker sounds.
    Audible,
    /// Both.
    Both,
}

impl Ring {
    /// The ring `zbm.console_bell` names: `none`, `visual`, `audible` or
    /// `both`.
    pub fn parse(name: &str) -> Option<Ring> {
        Some(match name {
            "none" => Ring::Off,
            "visual" => Ring::Visual,
            "audible" => Ring::Audible,
            "both" => Ring::Both,
            _ => return None,
        })
    }

    fn is_visual(self) -> bool {
        matches!(self, Ring::Visual | Ring::Both)
    }

    fn is_audible(self) -> bool {
        matches!(self, Ring::Audible | Ring::Both)
    }
}

/// The state of the bell.
#[derive(Debug, Default)]
pub(super) struct Bell {
    ring: Ring,
    /// Rung since the last tick: the border flashes from then.
    is_rung: bool,
    /// How often it rang since the speaker was last asked to sound.
    sounds: u32,
    /// Until when the border flashes.
    until: Option<Instant>,
    /// Whether the border was drawn for this flash.
    is_drawn: bool,
}

impl Bell {
    /// The bell rang: a flash that is on goes on as it is.
    pub fn ring(&mut self) {
        self.is_rung |= self.ring.is_visual() && self.until.is_none();
        self.sounds += u32::from(self.ring.is_audible());
    }

    /// Starts the flash rung for at `now`, or ends the one whose time came;
    /// says whether one ended.
    fn has_ended(&mut self, now: Instant) -> bool {
        if std::mem::take(&mut self.is_rung) {
            self.until = Some(now + FLASH);
            self.is_drawn = false;
        }
        if self.until.is_some_and(|until| until <= now) {
            self.until = None;
            return true;
        }
        false
    }

    /// When the flash ends, if one is on.
    pub fn at(&self) -> Option<Instant> {
        self.until
    }
}

impl Console {
    /// The console with the bell `ring`.
    #[must_use]
    pub fn with_bell(mut self, ring: Ring) -> Console {
        self.bell.ring = ring;
        self
    }

    /// How often the bell rang for the speaker since the last call.
    pub fn take_sounds(&mut self) -> u32 {
        std::mem::take(&mut self.bell.sounds)
    }

    /// Starts or ends the flash at `now`; one that ended is drawn over.
    pub(super) fn tick_bell(&mut self, now: Instant) {
        if self.bell.has_ended(now) {
            self.repaint();
        }
    }

    /// Draws the border of a flash that started; returns the cells drawn,
    /// one for the border.
    pub(super) fn draw_flash(&mut self) -> usize {
        if self.bell.until.is_none() || std::mem::replace(&mut self.bell.is_drawn, true) {
            return 0;
        }
        let (width, height) = self.frame.size();
        let thickness = (self.grid.cell().1 / 6).max(2);
        let colour = self.scheme.foreground;
        let sides = [
            Area {
                x: 0,
                y: 0,
                width,
                height: thickness,
            },
            Area {
                x: 0,
                y: height.saturating_sub(thickness),
                width,
                height: thickness,
            },
            Area {
                x: 0,
                y: 0,
                width: thickness,
                height,
            },
            Area {
                x: width.saturating_sub(thickness),
                y: 0,
                width: thickness,
                height,
            },
        ];
        for side in sides {
            self.frame.fill(side, colour);
        }
        1
    }
}
