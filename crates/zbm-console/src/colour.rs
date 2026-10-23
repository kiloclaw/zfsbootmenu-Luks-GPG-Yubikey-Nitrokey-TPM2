//! How a cell of the terminal is drawn: its colours in the Linux console's
//! palette, and the 256 colours and true colour beyond it.
//!
//! The defaults are the console's (grey on black), and bold draws the eight
//! base colours bright as the console does, so that programs written for
//! the kernel console look the same on this one.

use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, Rgb as TermRgb};
use zbm_display::frame::Rgb;
use zbm_display::grid::CellStyle;
use zbm_display::palette;

/// The pixels of an RGB colour of the terminal.
fn rgb(colour: TermRgb) -> Rgb {
    Rgb(colour.r, colour.g, colour.b)
}

/// The colour numbered `index` in the terminal's table: what a program set
/// with OSC 4, else the palette's (0 to 255), the defaults (256 and 257)
/// and the dim colours (259 to 266, 268). Bold default text is the
/// console's bright white (267).
pub fn by_index(colours: &Colors, index: usize) -> Rgb {
    if let Some(colour) = colours[index] {
        return rgb(colour);
    }
    let named = |number: usize| u8::try_from(number).map_or(Rgb::BLACK, palette::indexed);
    match index {
        0..=255 => named(index),
        256 | 258 => palette::FOREGROUND,
        267 => named(15),
        257 => palette::BACKGROUND,
        259..=266 => palette::dim(named(index - 259)),
        _ => palette::dim(palette::FOREGROUND),
    }
}

/// The pixels of `colour` in the terminal's table `colours`; `bold`
/// brightens the eight base colours.
pub fn resolve(colours: &Colors, colour: Color, bold: bool) -> Rgb {
    match colour {
        Color::Spec(colour) => rgb(colour),
        Color::Indexed(number) if bold && number < 8 => by_index(colours, usize::from(number) + 8),
        Color::Indexed(number) => by_index(colours, usize::from(number)),
        Color::Named(name) => {
            let name = if bold { name.to_bright() } else { name };
            by_index(colours, name as usize)
        }
    }
}

/// How `cell` is drawn: its colours, with bold, dim, inverse and hidden
/// as the console draws them, and underlined.
pub fn style(colours: &Colors, cell: &Cell) -> CellStyle {
    let flags = cell.flags;
    let bold = flags.contains(Flags::BOLD);
    let mut fg = resolve(colours, cell.fg, bold);
    let mut bg = resolve(colours, cell.bg, false);
    if flags.contains(Flags::DIM) {
        fg = palette::dim(fg);
    }
    if flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if flags.contains(Flags::HIDDEN) {
        fg = bg;
    }
    CellStyle {
        fg,
        bg,
        underline: flags.intersects(Flags::ALL_UNDERLINES),
    }
}

/// The style of the cursor drawn over `cell` as a block: its colours
/// swapped.
pub fn cursor(colours: &Colors, cell: &Cell) -> CellStyle {
    let style = style(colours, cell);
    CellStyle {
        fg: style.bg,
        bg: style.fg,
        underline: false,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use alacritty_terminal::vte::ansi::NamedColor;

    const DEFAULT_FG: Color = Color::Named(NamedColor::Foreground);

    fn cell(fg: Color, bg: Color, flags: Flags) -> Cell {
        Cell {
            fg,
            bg,
            flags,
            ..Cell::default()
        }
    }

    #[test]
    fn colours_are_the_console_s_with_256_and_true_colour() {
        let colours = Colors::default();
        let named = |name| resolve(&colours, Color::Named(name), false);

        assert_eq!(named(NamedColor::Foreground), Rgb(0xaa, 0xaa, 0xaa));
        assert_eq!(named(NamedColor::Background), Rgb::BLACK);
        assert_eq!(named(NamedColor::Yellow), Rgb(0xaa, 0x55, 0x00));
        assert_eq!(named(NamedColor::DimRed), Rgb(0x55, 0, 0));
        assert_eq!(named(NamedColor::DimForeground), Rgb(0x55, 0x55, 0x55));
        assert_eq!(named(NamedColor::Cursor), Rgb(0xaa, 0xaa, 0xaa));
        let bold = resolve(&colours, DEFAULT_FG, true);
        assert_eq!(bold, Rgb(0xff, 0xff, 0xff));
        let spec = Color::Spec(TermRgb { r: 1, g: 2, b: 3 });
        assert_eq!(resolve(&colours, spec, true), Rgb(1, 2, 3));
        assert_eq!(
            resolve(&colours, Color::Indexed(196), false),
            Rgb(255, 0, 0)
        );
    }

    #[test]
    fn bold_brightens_the_base_colours_and_a_program_s_colours_win() {
        let mut colours = Colors::default();
        assert_eq!(
            resolve(&colours, Color::Indexed(2), true),
            Rgb(0x55, 0xff, 0x55)
        );
        assert_eq!(
            resolve(&colours, Color::Named(NamedColor::Blue), true),
            Rgb(0x55, 0x55, 0xff)
        );
        assert_eq!(
            resolve(&colours, Color::Indexed(100), true),
            palette::indexed(100)
        );

        colours[4] = Some(TermRgb { r: 9, g: 9, b: 9 });
        assert_eq!(by_index(&colours, 4), Rgb(9, 9, 9));
    }

    #[test]
    fn attributes_change_the_colours_drawn() {
        let colours = Colors::default();
        let red = Color::Named(NamedColor::Red);
        let black = Color::Named(NamedColor::Background);

        let inverse = style(&colours, &cell(red, black, Flags::INVERSE));
        let dim = style(&colours, &cell(red, black, Flags::DIM));
        let hidden = style(&colours, &cell(red, black, Flags::HIDDEN));
        let underlined = style(&colours, &cell(red, black, Flags::DOUBLE_UNDERLINE));
        let over_cursor = cursor(&colours, &cell(DEFAULT_FG, black, Flags::UNDERLINE));

        assert_eq!((inverse.fg, inverse.bg), (Rgb::BLACK, Rgb(0xaa, 0, 0)));
        assert_eq!(dim.fg, Rgb(0x55, 0, 0));
        assert_eq!(hidden.fg, hidden.bg);
        assert!(underlined.underline && !dim.underline);
        assert_eq!(
            (over_cursor.fg, over_cursor.bg),
            (Rgb::BLACK, Rgb(0xaa, 0xaa, 0xaa))
        );
        assert!(!over_cursor.underline);
    }
}
