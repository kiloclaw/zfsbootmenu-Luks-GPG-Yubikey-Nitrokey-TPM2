//! Where the screen is on the display, and how large: as many cells as
//! fit, from the top left; or, while a smaller terminal is the one typed
//! on, as the serial console (ROADMAP.md UI9), that terminal's size,
//! centred, so that the programs draw for the terminal in use.

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::term::test::TermSize;
use zbm_display::frame::Area;

use super::Console;

impl Console {
    /// Holds the screen to `size` (columns, rows), centred on the display,
    /// or lets it fill the display again with `None`; the size to tell the
    /// programs, when it is another.
    pub fn hold_size(&mut self, size: Option<(u16, u16)>) -> Option<WindowSize> {
        if self.held == size {
            return None;
        }
        let before = self.size();
        self.held = size;
        self.place();
        (self.size() != before || size.is_some()).then(|| self.window_size())
    }

    /// The size the screen is held to, if it is.
    pub fn held_size(&self) -> Option<(u16, u16)> {
        self.held
    }

    /// Sizes the terminal for the grid and the display, held to
    /// [`Console::held_size`], and places its cells: from the top left
    /// when they fill the display, centred when held; the frame is drawn
    /// again as a whole.
    pub(super) fn place(&mut self) {
        let pixels = self.pixels();
        let (columns, rows) = self.grid.dimensions(pixels);
        let status = u16::from(self.has_status_line());
        let (mut columns, mut rows) = (columns.max(1), rows.saturating_sub(status).max(1));
        let mut origin = (0, 0);
        if let Some((held_columns, held_rows)) = self.held {
            columns = columns.min(held_columns.max(1));
            rows = rows.min(held_rows.max(1));
            let (width, height) = self.grid.cell();
            let used = (
                u32::from(columns) * width,
                u32::from(rows + status) * height,
            );
            origin = (
                pixels.0.saturating_sub(used.0) / 2,
                pixels.1.saturating_sub(used.1) / 2,
            );
        }
        self.grid.set_origin(origin);
        self.columns = columns;
        self.rows = rows;
        self.term
            .resize(TermSize::new(usize::from(columns), usize::from(rows)));
        self.forget_match();
        self.repaint();
        self.resize_cells();
        // The terminal's events of the resize are not for the programs.
        self.events.0.borrow_mut().clear();
    }

    /// The parts of the frame beyond the cells, which the background
    /// fills: on the right and below them, and on the left and above when
    /// they are centred.
    pub(super) fn margins(&self) -> [Area; 4] {
        let (width, height) = self.pixels();
        let (cell_width, cell_height) = self.grid.cell();
        let (left, top) = self.grid.grid().origin();
        let right = left + u32::from(self.columns) * cell_width;
        let bottom = top + u32::from(self.display_rows()) * cell_height;
        [
            Area {
                x: 0,
                y: 0,
                width: left,
                height,
            },
            Area {
                x: right,
                y: 0,
                width: width.saturating_sub(right),
                height,
            },
            Area {
                x: 0,
                y: 0,
                width,
                height: top,
            },
            Area {
                x: 0,
                y: bottom,
                width,
                height: height.saturating_sub(bottom),
            },
        ]
    }
}

#[cfg(test)]
mod tests;
