//! The screen as cells with their styles, for the serial console that the
//! console serves too (ROADMAP.md UI9): what the display shows, the
//! programs' terminal or the client's cells, with the bar over them and
//! the status line last.

use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::CursorShape;
use zbm_display::grid::CellStyle;

use super::Console;
use super::look::{look, reversed};
use crate::colour::Table;

/// A cell as shown: its text, a character and its combining marks, empty
/// for the second cell of a wide character; and its style.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyledCell {
    /// What the cell shows.
    pub text: String,
    /// Its colours, face and decoration.
    pub style: CellStyle,
}

/// The screen as shown: its rows of cells, and the cursor (column, row)
/// when it is shown.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Styled {
    /// The rows, the status line last.
    pub rows: Vec<Vec<StyledCell>>,
    /// Where the cursor is, when it is shown.
    pub cursor: Option<(u16, u16)>,
}

impl Console {
    /// The screen as shown, as cells with their styles.
    pub fn styled(&self) -> Styled {
        let colours = Table::new(self.term.colors(), self.scheme);
        let bar = reversed(colours);
        let (mut rows, cursor) = match &self.cells {
            Some(cells) => {
                let size = (usize::from(self.rows), usize::from(self.columns));
                let overlay = self.overlay();
                cells.styled(|row, column| overlay.char_at((row, column), size), bar)
            }
            None => (self.terminal_rows(), self.terminal_cursor()),
        };
        if let Some(status) = self.status_text() {
            rows.push(
                status
                    .iter()
                    .map(|&c| StyledCell {
                        text: c.to_string(),
                        style: bar,
                    })
                    .collect(),
            );
        }
        Styled { rows, cursor }
    }

    /// The rows of the programs' terminal as shown.
    fn terminal_rows(&self) -> Vec<Vec<StyledCell>> {
        let grid = self.term.grid();
        let shown = self.shown();
        let overlay = self.overlay();
        (0..grid.screen_lines())
            .map(|line| {
                (0..grid.columns())
                    .map(|column| {
                        let spacer = self
                            .flags(line, column)
                            .is_some_and(|flags| flags.contains(Flags::WIDE_CHAR_SPACER));
                        match look(&self.term, (line, column), None, shown, &overlay) {
                            Some((c, style, marks, _)) if !spacer => {
                                let mut text = c.to_string();
                                text.extend(marks);
                                StyledCell { text, style }
                            }
                            Some((_, style, ..)) => StyledCell {
                                text: String::new(),
                                style,
                            },
                            None => StyledCell {
                                text: " ".to_owned(),
                                style: self.plain(),
                            },
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// The scheme's default text on its default background.
    fn plain(&self) -> CellStyle {
        CellStyle {
            fg: self.scheme.foreground,
            bg: self.scheme.background,
            decoration: zbm_display::decor::Decoration::default(),
            face: zbm_display::faces::FontFace::Regular,
        }
    }

    /// Where the programs' cursor is, when it is shown and not scrolled
    /// off.
    fn terminal_cursor(&self) -> Option<(u16, u16)> {
        let cursor = self.term.renderable_content().cursor;
        if cursor.shape == CursorShape::Hidden {
            return None;
        }
        let offset = i32::try_from(self.term.grid().display_offset()).unwrap_or(0);
        let line = usize::try_from(cursor.point.line.0 + offset)
            .ok()
            .filter(|&line| line < self.screen_rows())?;
        Some((
            u16::try_from(cursor.point.column.0).ok()?,
            u16::try_from(line).ok()?,
        ))
    }
}

#[cfg(test)]
mod tests;
