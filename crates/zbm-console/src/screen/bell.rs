//! The bell (BEL): the frame's border flashes, the PC speaker sounds,
//! both, or neither (`zbm.console_bell`). When the flash ends, only the
//! cells under the border are drawn again (review fix 16).

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
    /// Whether the cells under the border of a flash that ended are to be
    /// drawn again.
    is_edged: bool,
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
            self.clear_flash();
        }
    }

    /// The flash ended: the margins beyond the cells are painted again now,
    /// and the cells under the border are drawn on the next frame, the
    /// first and last rows and columns, not the whole screen.
    fn clear_flash(&mut self) {
        let (width, height) = self.frame.size();
        let (cell_width, cell_height) = self.grid.cell();
        let right = u32::from(self.columns) * cell_width;
        let bottom = u32::from(self.display_rows()) * cell_height;
        let margins = [
            Area {
                x: right,
                y: 0,
                width: width.saturating_sub(right),
                height,
            },
            Area {
                x: 0,
                y: bottom,
                width,
                height: height.saturating_sub(bottom),
            },
        ];
        for margin in margins {
            self.frame.fill(margin, self.scheme.background);
        }
        let last = self.drawn_rows.len().saturating_sub(1);
        for line in [0, last] {
            if let Some(row) = self.drawn_rows.get_mut(line) {
                *row = None;
            }
        }
        self.bell.is_edged = true;
        self.redraw = true;
    }

    /// Draws the first and last cell of each row after a flash ended;
    /// returns the cells drawn.
    pub(super) fn draw_edges(&mut self, cursor: Option<super::Cursor>) -> usize {
        if !std::mem::take(&mut self.bell.is_edged) {
            return 0;
        }
        let last = usize::from(self.columns).saturating_sub(1);
        let rows = usize::from(self.display_rows());
        for line in 0..rows {
            self.draw_cell(line, 0, cursor);
            self.draw_cell(line, last, cursor);
        }
        rows * 2
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
