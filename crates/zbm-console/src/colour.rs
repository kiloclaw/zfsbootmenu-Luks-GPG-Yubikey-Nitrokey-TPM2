//! How a cell of the terminal is drawn: its colours in the Linux console's
//! palette, and the 256 colours and true colour beyond it.
//!
//! The defaults are the console's (grey on black), and bold draws the eight
//! base colours bright as the console does, so that programs written for
//! the kernel console look the same on this one.

use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb as TermRgb};
use zbm_display::decor::{Decoration, Underline};
use zbm_display::faces::FontFace;
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

/// The underline `flags` ask for (SGR 4, 4:2 to 4:5, 21).
fn underline(flags: Flags) -> Underline {
    [
        (Flags::UNDERCURL, Underline::Curly),
        (Flags::DOTTED_UNDERLINE, Underline::Dotted),
        (Flags::DASHED_UNDERLINE, Underline::Dashed),
        (Flags::DOUBLE_UNDERLINE, Underline::Double),
        (Flags::UNDERLINE, Underline::Single),
    ]
    .into_iter()
    .find_map(|(flag, kind)| flags.contains(flag).then_some(kind))
    .unwrap_or_default()
}

/// The lines drawn under and through `cell`, the underline in the colour
/// SGR 58 gave it; none when it is hidden.
fn decoration(colours: &Colors, cell: &Cell) -> Decoration {
    let flags = cell.flags;
    if flags.contains(Flags::HIDDEN) {
        return Decoration::default();
    }
    Decoration {
        underline: underline(flags),
        underline_colour: cell
            .underline_color()
            .map(|colour| resolve(colours, colour, false)),
        strikeout: flags.contains(Flags::STRIKEOUT),
    }
}

/// How `cell` is drawn: its colours, with bold, dim, inverse and hidden
/// as the console draws them, and its underline and strikeout.
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
        decoration: decoration(colours, cell),
        face: FontFace::of(bold, flags.contains(Flags::ITALIC)),
    }
}

/// How the screen is shown at a moment.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Shown {
    /// Whether it is reversed (DECSCNM): the colours of each cell swapped.
    pub reverse: bool,
    /// Whether blinking text is hidden.
    pub blink_off: bool,
}

/// `style`, of a cell of `flags`, as the screen is `shown`.
pub fn shown(mut style: CellStyle, flags: Flags, shown: Shown) -> CellStyle {
    if shown.reverse {
        std::mem::swap(&mut style.fg, &mut style.bg);
    }
    if shown.blink_off && flags.contains(Flags::BLINK) {
        style.fg = style.bg;
        style.decoration = Decoration::default();
    }
    style
}

/// The colour a program gave the cursor (OSC 12), until it reset it (OSC
/// 112).
pub fn cursor_colour(colours: &Colors) -> Option<Rgb> {
    colours[NamedColor::Cursor].map(rgb)
}

/// The style of the cursor drawn as a block over a cell of `style`: its
/// colours swapped, or the cursor's colour behind the character's
/// background as text.
pub fn cursor(colours: &Colors, style: CellStyle) -> CellStyle {
    CellStyle {
        fg: style.bg,
        bg: cursor_colour(colours).unwrap_or(style.fg),
        decoration: Decoration::default(),
        face: style.face,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

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
        let over_cursor = cursor(
            &colours,
            style(&colours, &cell(DEFAULT_FG, black, Flags::UNDERLINE)),
        );
        let hidden_line = style(
            &colours,
            &cell(red, black, Flags::HIDDEN | Flags::STRIKEOUT),
        );

        assert_eq!((inverse.fg, inverse.bg), (Rgb::BLACK, Rgb(0xaa, 0, 0)));
        assert_eq!(dim.fg, Rgb(0x55, 0, 0));
        assert_eq!(hidden.fg, hidden.bg);
        assert_eq!(underlined.decoration.underline, Underline::Double);
        assert_eq!(dim.decoration, Decoration::default());
        assert_eq!(hidden_line.decoration, Decoration::default());
        assert_eq!(
            (over_cursor.fg, over_cursor.bg),
            (Rgb::BLACK, Rgb(0xaa, 0xaa, 0xaa))
        );
        assert_eq!(over_cursor.decoration, Decoration::default());
        // Bold and italic text asks for its face.
        let face = |flags| style(&colours, &cell(red, black, flags)).face;
        assert_eq!(face(Flags::BOLD | Flags::ITALIC), FontFace::BoldItalic);
        assert_eq!(face(Flags::ITALIC), FontFace::Italic);
        assert_eq!(face(Flags::empty()), FontFace::Regular);

        // A program's cursor colour (OSC 12) is the block's.
        let mut coloured = Colors::default();
        coloured[NamedColor::Cursor] = Some(TermRgb {
            r: 0,
            g: 0xff,
            b: 0,
        });
        let green = cursor(
            &coloured,
            style(&coloured, &cell(DEFAULT_FG, black, Flags::empty())),
        );
        assert_eq!((green.fg, green.bg), (Rgb::BLACK, Rgb(0, 0xff, 0)));
        assert_eq!(cursor_colour(&colours), None);
    }

    #[test]
    fn underlines_strikeout_and_the_underline_s_colour() {
        let colours = Colors::default();
        let black = Color::Named(NamedColor::Background);
        let kinds = [
            (Flags::UNDERCURL | Flags::UNDERLINE, Underline::Curly),
            (Flags::DOTTED_UNDERLINE, Underline::Dotted),
            (Flags::DASHED_UNDERLINE, Underline::Dashed),
            (Flags::UNDERLINE, Underline::Single),
            (Flags::empty(), Underline::None),
        ];
        for (flags, kind) in kinds {
            let drawn = style(&colours, &cell(DEFAULT_FG, black, flags));
            assert_eq!(drawn.decoration.underline, kind, "{flags:?}");
        }

        let mut coloured = cell(DEFAULT_FG, black, Flags::UNDERLINE | Flags::STRIKEOUT);
        coloured.set_underline_color(Some(Color::Indexed(1)));
        let drawn = style(&colours, &coloured);
        assert_eq!(drawn.decoration.underline_colour, Some(Rgb(0xaa, 0, 0)));
        assert!(drawn.decoration.strikeout);
    }
}
