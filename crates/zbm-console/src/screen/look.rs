//! What a cell of the terminal shows, as drawn: its character and style,
//! what is being typed over it, and the cursor.

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::CursorShape;
use zbm_display::decor::Decoration;
use zbm_display::frame::{Area, Frame, Rgb};
use zbm_display::grid::CellStyle;

use super::{Cursor, Events};
use crate::colour::{self, Shown};

/// What the cell of `term` at (line, column) shows as the screen is
/// `shown`: its character, its style, its combining marks, and the shape
/// of `cursor` when it is over it.
pub(super) fn look(
    term: &Term<Events>,
    (line, column): (usize, usize),
    cursor: Option<Cursor>,
    shown: Shown,
) -> Option<(char, CellStyle, &[char], Option<CursorShape>)> {
    // The line shown on row `line`, above the screen while scrolled back.
    let offset = i32::try_from(term.grid().display_offset()).ok()?;
    let row = i32::try_from(line).ok()? - offset;
    let grid = term.grid();
    if line >= grid.screen_lines() || column >= grid.columns() {
        return None;
    }
    let cell = &grid[Line(row)][Column(column)];
    let colours = term.colors();
    // What is being typed, underlined from the cursor on.
    let previewed = cursor
        .filter(|cursor| cursor.line == line)
        .and_then(|cursor| {
            let chars = cursor.preview?;
            chars
                .chars()
                .get(column.checked_sub(cursor.column)?)
                .copied()
        });
    if let Some(c) = previewed {
        let style = colour::shown(colour::style(colours, cell), cell.flags, shown);
        let decoration = Decoration::underlined(true);
        return Some((
            c,
            CellStyle {
                decoration,
                ..style
            },
            &[],
            None,
        ));
    }
    let shape = cursor
        .filter(|cursor| cursor.line == line && cursor.column == column)
        .map(|cursor| cursor.shape)
        .filter(|&shape| shape != CursorShape::Hidden);

    let style = colour::shown(colour::style(colours, cell), cell.flags, shown);
    let style = if shape == Some(CursorShape::Block) {
        colour::cursor(colours, style)
    } else {
        style
    };
    let c = if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
        ' '
    } else {
        cell.c
    };
    Some((c, style, cell.zerowidth().unwrap_or_default(), shape))
}

/// Draws a cursor of `shape` other than the block, which swaps the cell's
/// colours, over `area`, in `colour`: an underline, a bar on the left, or
/// a hollow box, as thick as an eighth of the cell's width.
pub(super) fn draw_cursor(frame: &mut Frame, area: Area, shape: CursorShape, colour: Rgb) {
    let thickness = (area.width / 8).max(1);
    let bottom = Area {
        y: area.y + area.height.saturating_sub(thickness),
        height: thickness.min(area.height),
        ..area
    };
    let left = Area {
        width: thickness.min(area.width),
        ..area
    };
    let parts = match shape {
        CursorShape::Underline => vec![bottom],
        CursorShape::Beam => vec![left],
        CursorShape::HollowBlock => vec![
            bottom,
            left,
            Area {
                height: bottom.height,
                ..area
            },
            Area {
                x: area.x + area.width.saturating_sub(thickness),
                ..left
            },
        ],
        CursorShape::Block | CursorShape::Hidden => Vec::new(),
    };
    for part in parts {
        frame.fill(part, colour);
    }
}
