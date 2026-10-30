//! The font's size while the console runs: Ctrl+Alt+`=` (or `+`) and
//! Ctrl+Alt+`-` ask for the next size up and down, Ctrl+Alt+`0` for the
//! size the console started with, and `zbm console font` for a size. The
//! console then draws in a new grid, which the caller makes, and the
//! programs are told the new size.

use alacritty_terminal::term::test::TermSize;
use zbm_display::frame::Area;
use zbm_display::grid::Grid;
use zbm_display::keys::{Key, KeyPress};
use zbm_display::render::CellRenderer;

use super::Console;
use crate::scheme::Scheme;

/// The fewest columns a grid may have to be taken a size up.
pub const MIN_COLUMNS: u16 = 20;

/// The fewest rows a grid may have to be taken a size up.
pub const MIN_ROWS: u16 = 5;

/// The cell heights tried, in pixels, from the smallest.
const HEIGHTS: [u16; 22] = [
    8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 32, 36, 40, 44, 48, 56, 64, 72, 80, 96, 112,
];

/// A change of the font's size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Zoom {
    /// The next size up.
    Bigger,
    /// The next size down.
    Smaller,
    /// The size the console started with.
    Start,
    /// Cells this many pixels high.
    Height(u16),
}

impl Zoom {
    /// The zoom `zbm console font` asks for with `text`: `+` or `-`,
    /// `default` for the size the console started with, or a height in
    /// pixels.
    pub fn parse(text: &str) -> Option<Zoom> {
        match text {
            "+" => Some(Zoom::Bigger),
            "-" => Some(Zoom::Smaller),
            "default" => Some(Zoom::Start),
            _ => text
                .parse()
                .ok()
                .filter(|height| (6..=200).contains(height))
                .map(Zoom::Height),
        }
    }

    /// The zoom `press` asks for: Ctrl+Alt with `=`, `+`, `-` or `0`.
    pub fn of(press: KeyPress) -> Option<Zoom> {
        if !press.ctrl || !press.alt {
            return None;
        }
        match press.key {
            Key::Char('=' | '+') => Some(Zoom::Bigger),
            Key::Char('-' | '_') => Some(Zoom::Smaller),
            Key::Char('0' | ')') => Some(Zoom::Start),
            _ => None,
        }
    }

    /// The cell heights to try, in order, from cells `current` pixels high:
    /// the larger ones up, the smaller ones down.
    pub fn heights(self, current: u32) -> Vec<Option<u16>> {
        match self {
            Zoom::Bigger => HEIGHTS
                .iter()
                .filter(|&&height| u32::from(height) > current)
                .map(|&height| Some(height))
                .collect(),
            Zoom::Smaller => HEIGHTS
                .iter()
                .rev()
                .filter(|&&height| u32::from(height) < current)
                .map(|&height| Some(height))
                .collect(),
            Zoom::Start => vec![None],
            // A request that could not be read: none.
            Zoom::Height(0) => Vec::new(),
            Zoom::Height(height) => vec![Some(height)],
        }
    }

    /// Whether `grid`, for a display of `pixels`, is what the zoom asks
    /// from cells `current` pixels high: larger, with room enough, or
    /// smaller; any for the others, whose font may only come near the
    /// height asked.
    pub fn is_met_by(self, grid: &Grid, pixels: (u32, u32), current: u32) -> bool {
        let height = grid.cell().1;
        let (columns, rows) = grid.dimensions(pixels);
        match self {
            Zoom::Bigger => height > current && columns >= MIN_COLUMNS && rows >= MIN_ROWS,
            Zoom::Smaller => height < current,
            Zoom::Start | Zoom::Height(_) => columns > 0 && rows > 0,
        }
    }
}

impl Console {
    /// Takes `press` when it changes the font's size; `Some(press)` when
    /// it does not.
    pub(super) fn take_zoom_key(&mut self, press: KeyPress) -> Option<KeyPress> {
        match Zoom::of(press) {
            Some(zoom) => {
                self.zoom = Some(zoom);
                None
            }
            None => Some(press),
        }
    }

    /// The change of the font's size asked for with the keys, once.
    pub fn take_zoom(&mut self) -> Option<Zoom> {
        self.zoom.take()
    }

    /// The font the console draws with: its name, its cells' size and how
    /// many, as `ter-v32b.psf 16x32, 80x25`.
    pub fn font_text(&self) -> String {
        let (width, height) = self.grid.cell();
        let (columns, rows) = self.size();
        let name = self.grid.name();
        format!("{name} {width}x{height}, {columns}x{rows}")
    }

    /// Draws in `grid` from now on: the terminal gets as many cells as fit
    /// the display, its lines rewrapped, and the frame is drawn again.
    pub fn set_grid(&mut self, grid: Grid) {
        let (columns, rows) = grid.dimensions(self.pixels());
        let status = u16::from(self.has_status_line());
        let (columns, rows) = (columns.max(1), rows.saturating_sub(status).max(1));
        self.grid = CellRenderer::new(grid);
        self.warm();
        self.columns = columns;
        self.rows = rows;
        self.term
            .resize(TermSize::new(usize::from(columns), usize::from(rows)));
        self.forget_match();
        self.repaint();
        // The terminal's events of the resize are not for the programs.
        self.events.0.borrow_mut().clear();
    }
}

impl Console {
    /// The console in the colour scheme `scheme`, its frame painted with
    /// the scheme's background.
    #[must_use]
    pub fn with_scheme(mut self, scheme: &'static Scheme) -> Console {
        self.scheme = scheme;
        self.repaint();
        self
    }

    /// Paints the frame with the scheme's background, to be drawn whole
    /// again.
    /// Blanks the screen (C64): the frame black, all of it to be
    /// presented; the programs' output is still drawn into it meanwhile.
    pub fn blank(&mut self) {
        let (width, height) = self.frame.size();
        let area = Area {
            x: 0,
            y: 0,
            width,
            height,
        };
        self.frame.fill(area, zbm_display::frame::Rgb::BLACK);
    }

    /// Shows the screen again after blanking: drawn again as a whole.
    pub fn unblank(&mut self) {
        self.repaint();
    }

    /// How long without a key blanks the screen, if it does.
    pub fn blank_after(&self) -> Option<std::time::Duration> {
        self.blank_after
    }

    /// The console, blanking after `after` without a key (C64).
    #[must_use]
    pub fn with_blank(mut self, after: Option<std::time::Duration>) -> Console {
        self.blank_after = after;
        self
    }

    pub(super) fn repaint(&mut self) {
        let (width, height) = self.frame.size();
        let area = Area {
            x: 0,
            y: 0,
            width,
            height,
        };
        self.frame.fill(area, self.scheme.background);
        self.drawn_rows = vec![None; usize::from(self.display_rows())];
        self.redraw = true;
    }

    /// The colour scheme.
    pub fn scheme(&self) -> &'static Scheme {
        self.scheme
    }
}
