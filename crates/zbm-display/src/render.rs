//! The one renderer of cells (ROADMAP.md UI4, the console's C71 and C76):
//! a [`Grid`] whose cells are drawn once for each character, style and
//! width, then copied into the frame row by row, instead of computing each
//! pixel again. The console's terminal and the menu's cells both draw
//! through it.
//!
//! A cell that a neighbour's glyph reaches into, as italic letters of a
//! TrueType font may, is drawn without the cache: what it shows depends on
//! the neighbour too. Combining marks are drawn over the cell, as before.
//! The cache holds at most [`CAPACITY`] cells, and starts again when full.

use std::collections::HashMap;

use crate::frame::{Area, Frame, Rgb};
use crate::grid::{CellStyle, Grid, Neighbour, Text};

/// The most cells the cache holds: 2 MiB with the largest console font.
pub const CAPACITY: usize = 1024;

/// What a cached cell is drawn from.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Key {
    c: char,
    style: CellStyle,
    span: u16,
}

/// Draws cells of a grid into frames, through a cache of drawn cells.
#[derive(Debug)]
pub struct CellRenderer {
    grid: Grid,
    /// The pixels of each cell drawn, row by row.
    cells: HashMap<Key, Vec<u32>>,
    /// Where a cell is drawn before it is cached; made again only when
    /// the cell's size changes.
    scratch: Frame,
}

impl CellRenderer {
    /// The renderer of `grid`, with an empty cache.
    pub fn new(grid: Grid) -> CellRenderer {
        let (width, height) = grid.cell();
        CellRenderer {
            grid,
            cells: HashMap::new(),
            scratch: Frame::new(width, height),
        }
    }

    /// The grid it draws with.
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// Draws the cell at column 0, row 0 at `origin` on the frame from now
    /// on ([`Grid::set_origin`]); the cells cached stay, as they do not
    /// depend on where they go.
    pub fn set_origin(&mut self, origin: (u32, u32)) {
        self.grid.set_origin(origin);
    }

    /// The cell on the frame, as (width, height) in pixels.
    pub fn cell(&self) -> (u32, u32) {
        self.grid.cell()
    }

    /// The pixels of the cell at `column`, `row`.
    pub fn area(&self, column: u16, row: u16) -> Area {
        self.grid.area(column, row)
    }

    /// The font's name in messages.
    pub fn name(&self) -> &str {
        self.grid.name()
    }

    /// How many columns and rows of cells fit in a frame of `size`.
    pub fn dimensions(&self, size: (u32, u32)) -> (u16, u16) {
        self.grid.dimensions(size)
    }

    /// How many cells the cache holds.
    pub fn cached(&self) -> usize {
        self.cells.len()
    }

    /// Draws `c` in the cell at `column`, `row`.
    pub fn draw(&mut self, frame: &mut Frame, column: u16, row: u16, c: char, style: CellStyle) {
        self.draw_text(frame, column, row, Text { c, style, span: 1 }, &[]);
    }

    /// Draws `text` in the cells from `column`, `row`: from the cache,
    /// unless a glyph of `neighbours` reaches into them.
    pub fn draw_text(
        &mut self,
        frame: &mut Frame,
        column: u16,
        row: u16,
        text: Text,
        neighbours: &[Neighbour],
    ) {
        let span = text.span.max(1);
        if neighbours
            .iter()
            .any(|neighbour| self.grid.is_reaching(neighbour, span))
        {
            self.grid.draw_text(frame, column, row, text, neighbours);
            return;
        }
        let key = Key {
            c: text.c,
            style: text.style,
            span,
        };
        let cell = self.grid.area(column, row);
        let area = Area {
            width: cell.width * u32::from(span),
            ..cell
        };
        if let Some(pixels) = self.cells.get(&key) {
            frame.blit(area, pixels);
            return;
        }
        let pixels = self.rasterize(key);
        frame.blit(area, &pixels);
        self.cells.insert(key, pixels);
    }

    /// Draws the combining mark `mark` in `colour` over the cell at
    /// `column`, `row`.
    pub fn draw_mark(&mut self, frame: &mut Frame, column: u16, row: u16, mark: char, colour: Rgb) {
        self.grid.draw_mark(frame, column, row, mark, colour);
    }

    /// Draws `chars` in `style` into the cache, so that the first frame
    /// that shows them only copies them: ASCII and the lines of boxes, in
    /// the screen's colours, at start.
    pub fn warm(&mut self, chars: impl IntoIterator<Item = char>, style: CellStyle) {
        for c in chars {
            let key = Key { c, style, span: 1 };
            if !self.cells.contains_key(&key) {
                let pixels = self.rasterize(key);
                self.cells.insert(key, pixels);
            }
        }
    }

    /// The pixels of the cell `key`, drawn by the grid, row by row; the
    /// cache starts again first when it is full.
    fn rasterize(&mut self, key: Key) -> Vec<u32> {
        if self.cells.len() >= CAPACITY {
            self.cells.clear();
        }
        let (width, height) = self.grid.cell();
        let width = width * u32::from(key.span);
        if self.scratch.size() != (width, height) {
            self.scratch = Frame::new(width, height);
        }
        let text = Text {
            c: key.c,
            style: key.style,
            span: key.span,
        };
        let cell = Area {
            x: 0,
            y: 0,
            width: width / u32::from(key.span.max(1)),
            height,
        };
        self.grid.draw_text_in(&mut self.scratch, cell, text, &[]);
        (0..height)
            .filter_map(|y| self.scratch.row(y))
            .flatten()
            .copied()
            .collect()
    }
}

/// The characters drawn into the cache at start: printable ASCII and the
/// lines and corners of boxes.
pub fn warm_chars() -> impl Iterator<Item = char> {
    (' '..='~').chain("─│┌┐└┘├┤┬┴┼".chars())
}

#[cfg(test)]
mod tests;
