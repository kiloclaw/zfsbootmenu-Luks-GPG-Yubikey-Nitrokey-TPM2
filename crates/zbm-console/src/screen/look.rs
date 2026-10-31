//! What a cell of the terminal shows, as drawn: its character and style,
//! what is being typed over it, and the cursor.

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
use zbm_display::decor::Decoration;
use zbm_display::faces::FontFace;
use zbm_display::frame::{Area, Frame, Rgb};
use zbm_display::grid::CellStyle;

use super::seek::Overlay;
use super::{Cursor, Events};
use crate::colour::{self, Shown, Table};

/// What the cell of `term` at (line, column) shows as the screen is
/// `shown`: its character, its style, its combining marks, and the shape
/// of `cursor` when it is over it; or what `overlay` draws there.
pub(super) fn look<'a>(
    term: &'a Term<Events>,
    (line, column): (usize, usize),
    cursor: Option<Cursor>,
    shown: Shown,
    overlay: &Overlay,
) -> Option<(char, CellStyle, &'a [char], Option<CursorShape>)> {
    // The line shown on row `line`, above the screen while scrolled back.
    let offset = i32::try_from(term.grid().display_offset()).ok()?;
    let row = i32::try_from(line).ok()? - offset;
    let grid = term.grid();
    let colours = Table::new(term.colors(), shown.scheme);
    // The search bar, the label and the status line, in the default
    // colours reversed.
    let size = (grid.screen_lines(), grid.columns());
    if column < grid.columns()
        && let Some(c) = overlay.char_at((line, column), size)
    {
        return Some((c, reversed(colours), &[], None));
    }
    if line >= grid.screen_lines() || column >= grid.columns() {
        return None;
    }
    let cell = &grid[Line(row)][Column(column)];
    // What is being typed, underlined from the cursor on.
    if let Some(c) = previewed(cursor, (line, column)) {
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

    let is_found = overlay.has_found(Point::new(Line(row), Column(column)));
    let style = styled(colours, cell, shown, is_found, shape);
    let c = if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
        ' '
    } else {
        cell.c
    };
    Some((c, style, cell.zerowidth().unwrap_or_default(), shape))
}

/// How `cell` is drawn as the screen is `shown`: in a match found, under
/// a cursor of `shape`.
fn styled(
    colours: Table<'_>,
    cell: &Cell,
    shown: Shown,
    is_found: bool,
    shape: Option<CursorShape>,
) -> CellStyle {
    let style = colour::shown(colour::style(colours, cell), cell.flags, shown);
    let style = if is_found {
        found(colours, style)
    } else {
        style
    };
    if shape == Some(CursorShape::Block) {
        colour::cursor(colours, style)
    } else {
        style
    }
}

/// The character being typed that `cursor` shows at (line, column), if
/// any: the preview starts at the cursor.
fn previewed(cursor: Option<Cursor>, (line, column): (usize, usize)) -> Option<char> {
    let cursor = cursor.filter(|cursor| cursor.line == line)?;
    let preview = cursor.preview?;
    preview
        .chars()
        .get(column.checked_sub(cursor.column)?)
        .copied()
}

/// The style of the search bar and the label: the default colours
/// reversed.
pub(super) fn reversed(colours: Table<'_>) -> CellStyle {
    let named = |name| colour::resolve(colours, Color::Named(name), false);
    CellStyle {
        fg: named(NamedColor::Background),
        bg: named(NamedColor::Foreground),
        decoration: Decoration::default(),
        face: FontFace::Regular,
    }
}

/// `style` in a match found: black on yellow.
fn found(colours: Table<'_>, style: CellStyle) -> CellStyle {
    CellStyle {
        fg: colour::by_index(colours, 0),
        bg: colour::by_index(colours, 3),
        ..style
    }
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
