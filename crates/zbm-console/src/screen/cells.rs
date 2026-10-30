//! The cell surface (ADR 0004, UI6): the cells the menu sends on the client
//! channel, drawn with the terminal's renderer in place of the terminal
//! while the menu has the screen. The terminal goes on reading what the
//! programs write meanwhile, and is drawn whole again once the menu lends
//! the screen or goes.
//!
//! A frame's cells may come in several messages: they are drawn once the
//! frame is whole, which its cursor says, so that half a frame never
//! shows. Only the cells that changed are drawn, with the ones beside
//! them, whose glyphs may reach into them.

use zbm_display::channel::Run;
use zbm_display::faces::FontFace;
use zbm_display::frame::{Frame, Rgb};
use zbm_display::grid::{CellStyle, Neighbour, Text};
use zbm_display::render::CellRenderer;

use super::Console;

/// A cell of the surface: its text, a grapheme, or empty for the second
/// cell of a wide character; and its style.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Cell {
    text: String,
    style: CellStyle,
}

/// The cells the client drew, and which of them are to be drawn.
#[derive(Debug)]
pub(super) struct Surface {
    columns: u16,
    rows: u16,
    /// The cells, row after row.
    cells: Vec<Cell>,
    /// Whether each cell changed since it was drawn.
    changed: Vec<bool>,
    /// The cells to draw in the next frame, kept so that drawing allocates
    /// nothing.
    dirty: Vec<bool>,
    /// The client's cursor, as its last whole frame put it.
    cursor: Option<(u16, u16)>,
    /// Where the cursor was drawn.
    drawn_cursor: Option<(u16, u16)>,
    /// Whether a whole frame waits to be drawn.
    is_ready: bool,
}

impl Surface {
    /// A surface of `columns` and `rows` of blank cells in `blank`.
    fn new((columns, rows): (u16, u16), blank: CellStyle) -> Surface {
        let count = usize::from(columns) * usize::from(rows);
        let cell = Cell {
            text: " ".to_owned(),
            style: blank,
        };
        Surface {
            columns,
            rows,
            cells: vec![cell; count],
            changed: vec![true; count],
            dirty: vec![false; count],
            cursor: None,
            drawn_cursor: None,
            is_ready: false,
        }
    }

    /// The place of the cell at `column` and `row`, if there is one.
    fn index(&self, column: u16, row: u16) -> Option<usize> {
        (column < self.columns && row < self.rows)
            .then(|| usize::from(row) * usize::from(self.columns) + usize::from(column))
    }

    /// The text of the cell at `column` and `row`, if there is one.
    fn text(&self, column: u16, row: u16) -> Option<&str> {
        let index = self.index(column, row)?;
        self.cells.get(index).map(|cell| cell.text.as_str())
    }

    /// Takes the cells of `runs`: those beyond the surface are dropped, and
    /// control characters shown as spaces.
    fn put(&mut self, runs: &[Run]) {
        for run in runs {
            let columns = (run.column..self.columns).zip(&run.cells);
            for (column, text) in columns {
                let Some(index) = self.index(column, run.row) else {
                    break;
                };
                let (Some(cell), Some(changed)) =
                    (self.cells.get_mut(index), self.changed.get_mut(index))
                else {
                    break;
                };
                let is_same =
                    cell.style == run.style && cell.text.chars().eq(text.chars().map(printable));
                if !is_same {
                    cell.text.clear();
                    cell.text.extend(text.chars().map(printable));
                    cell.style = run.style;
                    *changed = true;
                }
            }
        }
    }

    /// The frame is whole, with the cursor at (column, row), or hidden.
    fn end_frame(&mut self, cursor: Option<(u16, u16)>) {
        self.cursor = cursor;
        self.is_ready = true;
    }

    /// Marks the cells to draw: all of them when `whole`; otherwise those
    /// that changed with the ones beside them in their row, and the cells
    /// of the cursor, where it was and where it is.
    fn mark_dirty(&mut self, whole: bool) {
        if whole {
            self.dirty.fill(true);
            self.changed.fill(false);
            return;
        }
        self.dirty.fill(false);
        let columns = usize::from(self.columns).max(1);
        for (index, changed) in self.changed.iter_mut().enumerate() {
            if !std::mem::take(changed) {
                continue;
            }
            let column = index % columns;
            let first = index - column.min(1);
            let last = (index + 1).min(index - column + columns - 1);
            for dirty in self.dirty.get_mut(first..=last).unwrap_or_default() {
                *dirty = true;
            }
        }
        for (column, row) in [self.drawn_cursor, self.cursor].into_iter().flatten() {
            if let Some(index) = self.index(column, row)
                && let Some(dirty) = self.dirty.get_mut(index)
            {
                *dirty = true;
            }
        }
    }

    /// Draws the frame that waits, if one does, or all of it when `whole`,
    /// with `renderer` into `frame`; returns the cells drawn.
    fn draw(&mut self, renderer: &mut CellRenderer, frame: &mut Frame, whole: bool) -> usize {
        if !whole && !std::mem::take(&mut self.is_ready) {
            return 0;
        }
        self.mark_dirty(whole);
        let mut drawn = 0;
        let columns = usize::from(self.columns).max(1);
        for (index, &dirty) in self.dirty.iter().enumerate() {
            let (Ok(column), Ok(row)) = (
                u16::try_from(index % columns),
                u16::try_from(index / columns),
            ) else {
                break;
            };
            if dirty {
                self.draw_cell(renderer, frame, (column, row));
                drawn += 1;
            }
        }
        self.drawn_cursor = self.cursor;
        drawn
    }

    /// Draws the cell at (column, row): a wide character over both of its
    /// cells, from either; under the cursor, in its colours reversed.
    fn draw_cell(&self, renderer: &mut CellRenderer, frame: &mut Frame, (column, row): (u16, u16)) {
        let is_second_half = column > 0
            && self.text(column, row) == Some("")
            && self
                .text(column - 1, row)
                .is_some_and(|text| !text.is_empty());
        let column = if is_second_half { column - 1 } else { column };
        let Some(cell) = self
            .index(column, row)
            .and_then(|index| self.cells.get(index))
        else {
            return;
        };
        let mut chars = cell.text.chars();
        let c = chars.next().unwrap_or(' ');
        let span = if self.text(column + 1, row) == Some("") {
            2
        } else {
            1
        };
        let style = if self.cursor == Some((column, row)) {
            CellStyle {
                fg: cell.style.bg,
                bg: cell.style.fg,
                ..cell.style
            }
        } else {
            cell.style
        };
        let (neighbours, count) = self.neighbours((column, row), span, renderer.cell().0);
        let text = Text { c, style, span };
        let neighbours = neighbours.get(..count).unwrap_or_default();
        renderer.draw_text(frame, column, row, text, neighbours);
        // Combining marks are drawn over the character they follow.
        for mark in chars {
            renderer.draw_mark(frame, column, row, mark, style.fg);
        }
    }

    /// The characters beside the cells from (column, row), `span` wide,
    /// whose glyphs may reach into them: the one before, from the start of
    /// its cells, and the one after; in cells `width` pixels wide.
    fn neighbours(
        &self,
        (column, row): (u16, u16),
        span: u16,
        width: u32,
    ) -> ([Neighbour; 2], usize) {
        let width = i64::from(width);
        let none = Neighbour {
            c: ' ',
            fg: Rgb::BLACK,
            face: FontFace::Regular,
            offset: 0,
        };
        let mut found = [none; 2];
        let mut count = 0;
        let before = column.checked_sub(1).map(|left| {
            if self.text(left, row) == Some("") {
                (left.saturating_sub(1), -2 * width)
            } else {
                (left, -width)
            }
        });
        let after = column
            .checked_add(span)
            .map(|right| (right, width * i64::from(span)));
        for (at, offset) in before.into_iter().chain(after) {
            let Some(cell) = self.index(at, row).and_then(|index| self.cells.get(index)) else {
                continue;
            };
            if let Some(c) = cell.text.chars().next().filter(|&c| c != ' ')
                && let Some(slot) = found.get_mut(count)
            {
                *slot = Neighbour {
                    c,
                    fg: cell.style.fg,
                    face: cell.style.face,
                    offset,
                };
                count += 1;
            }
        }
        (found, count)
    }

    /// Gives `each` the characters of each row, `None` at its end, and
    /// those of `over` where it draws over a cell.
    fn chars(
        &self,
        over: impl Fn(usize, usize) -> Option<char>,
        mut each: impl FnMut(Option<char>),
    ) {
        let columns = usize::from(self.columns).max(1);
        for (row, cells) in self.cells.chunks(columns).enumerate() {
            for (column, cell) in cells.iter().enumerate() {
                if let Some(c) = over(row, column) {
                    each(Some(c));
                } else {
                    cell.text.chars().for_each(|c| each(Some(c)));
                }
            }
            each(None);
        }
    }
}

impl Drop for Surface {
    /// What the menu drew is wiped.
    fn drop(&mut self) {
        for cell in &mut self.cells {
            zbm_sys::wipe_vec(&mut std::mem::take(&mut cell.text).into_bytes());
        }
    }
}

/// `c`, or a space for a control character, which has no glyph and would
/// end the mirror's lines and screens.
fn printable(c: char) -> char {
    if c.is_control() { ' ' } else { c }
}

impl Console {
    /// Shows the client's cells from now on, blank until its first frame,
    /// as it asks when it takes the screen.
    pub fn show_cells(&mut self) {
        let blank = CellStyle {
            fg: self.scheme.foreground,
            bg: self.scheme.background,
            decoration: zbm_display::decor::Decoration::default(),
            face: FontFace::Regular,
        };
        self.cells = Some(Surface::new((self.columns, self.rows), blank));
        self.redraw = true;
    }

    /// Shows the terminal again, drawn whole, as when the client lends the
    /// screen or is gone; its cells are wiped.
    pub fn show_terminal(&mut self) {
        if self.cells.take().is_some() {
            self.repaint();
        }
    }

    /// Whether the client's cells are shown.
    pub fn is_showing_cells(&self) -> bool {
        self.cells.is_some()
    }

    /// Takes the client's `runs` of cells, shown once its frame is whole.
    pub fn put_cells(&mut self, runs: &[Run]) {
        if let Some(cells) = &mut self.cells {
            cells.put(runs);
        }
    }

    /// The client's frame is whole, its cursor at (column, row), or hidden.
    pub fn end_cells_frame(&mut self, cursor: Option<(u16, u16)>) {
        if let Some(cells) = &mut self.cells {
            cells.end_frame(cursor);
        }
    }

    /// The client's cells, blank, in the console's new size, as after the
    /// font's size changed: the client draws them whole.
    pub(super) fn resize_cells(&mut self) {
        if self.cells.is_some() {
            self.show_cells();
        }
    }

    /// Draws the client's frame that waits, if one does, and the search
    /// bar and the status line over it; returns the cells drawn.
    pub(super) fn draw_cells(&mut self) -> usize {
        // What the programs write meanwhile is drawn whole later.
        self.term.reset_damage();
        let overlay = self.overlay();
        let is_new_bar = overlay.bar != self.overlay.bar;
        let is_new_status = overlay.status != self.overlay.status;
        self.overlay = overlay;
        let whole = std::mem::take(&mut self.redraw) | is_new_bar;
        let Some(cells) = &mut self.cells else {
            return 0;
        };
        let drawn = cells.draw(&mut self.grid, &mut self.frame, whole);
        let over = if drawn > 0 || is_new_status {
            self.draw_bars()
        } else {
            0
        };
        drawn + over + self.draw_flash()
    }

    /// Draws the bar on the last row and the status line under it, where
    /// they are; returns the cells drawn.
    fn draw_bars(&mut self) -> usize {
        let size = (usize::from(self.rows), usize::from(self.columns));
        let mut drawn = 0;
        for line in [size.0.saturating_sub(1), size.0] {
            for column in 0..size.1 {
                if self.overlay.char_at((line, column), size).is_some() {
                    self.draw_cell(line, column, None);
                    drawn += 1;
                }
            }
        }
        drawn
    }

    /// Gives `each` the characters of the client's cells as shown, `None`
    /// at the end of each row, with the bar over them; then the status
    /// line's. Nothing without them.
    pub(super) fn cells_chars(&self, mut each: impl FnMut(Option<char>)) {
        let Some(cells) = &self.cells else {
            return;
        };
        let size = (usize::from(self.rows), usize::from(self.columns));
        let overlay = self.overlay();
        cells.chars(
            |row, column| overlay.char_at((row, column), size),
            &mut each,
        );
        if let Some(status) = self.status_text() {
            for &c in status {
                each(Some(c));
            }
            each(None);
        }
    }
}

#[cfg(test)]
mod tests;
