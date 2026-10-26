//! Text on the frame: a grid of character cells drawn with a console font,
//! the image's Terminus or Spleen fonts that the kernel console uses too,
//! at the size `zbm console-autosize` chooses, doubled where it says so;
//! or with a TrueType font ([`Outline`]) at that size, blended between the
//! cell's colours.

use std::collections::HashMap;

use zbm_types::psf::Font;

use crate::boxes;
use crate::decor::{self, Decoration};
use crate::frame::{Area, Frame, Rgb};
use crate::outline::Outline;

/// How a cell is drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellStyle {
    /// The colour of the character.
    pub fg: Rgb,
    /// The colour around it.
    pub bg: Rgb,
    /// The lines drawn under or through it.
    pub decoration: Decoration,
}

/// What the characters are drawn with.
#[derive(Debug)]
enum Face {
    /// A console font, each of its pixels `scale` pixels wide and high.
    Bitmap {
        font: Font,
        scale: u32,
        /// The glyph of each character drawn so far: finding one searches
        /// the font's whole table.
        glyphs: HashMap<char, Option<usize>>,
    },
    /// A TrueType or OpenType font, with box drawing and block characters
    /// drawn to fill their cells ([`boxes`]).
    Outline(Box<Outline>),
}

/// A font, which draws characters into cells of a frame.
#[derive(Debug)]
pub struct Grid {
    face: Face,
    name: String,
}

impl Grid {
    /// Draws with the console font `font`, each of its pixels `scale` pixels
    /// wide and high (2 for a font doubled as `setfont -d` doubles it).
    pub fn new(font: Font, scale: u32) -> Grid {
        Grid {
            face: Face::Bitmap {
                font,
                scale: scale.max(1),
                glyphs: HashMap::new(),
            },
            name: "psf".to_owned(),
        }
    }

    /// Draws with the TrueType or OpenType font `outline`.
    pub fn outline(outline: Outline) -> Grid {
        Grid {
            face: Face::Outline(Box::new(outline)),
            name: "ttf".to_owned(),
        }
    }

    /// The grid, named `name` (its font's file) in messages.
    #[must_use]
    pub fn named(mut self, name: &str) -> Grid {
        name.clone_into(&mut self.name);
        self
    }

    /// The font's name in messages.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The cell on the frame, as (width, height) in pixels.
    pub fn cell(&self) -> (u32, u32) {
        match &self.face {
            Face::Bitmap { font, scale, .. } => {
                let (width, height) = font.cell();
                (width * scale, height * scale)
            }
            Face::Outline(outline) => outline.cell(),
        }
    }

    /// How many columns and rows of cells fit in a frame of `size`.
    pub fn dimensions(&self, size: (u32, u32)) -> (u16, u16) {
        let (width, height) = self.cell();
        let fit = |pixels: u32, cell: u32| u16::try_from(pixels / cell.max(1)).unwrap_or(u16::MAX);
        (fit(size.0, width), fit(size.1, height))
    }

    /// The pixels of the cell at `column`, `row`.
    pub fn area(&self, column: u16, row: u16) -> Area {
        let (width, height) = self.cell();
        Area {
            x: u32::from(column) * width,
            y: u32::from(row) * height,
            width,
            height,
        }
    }

    /// Draws `c` in the cell at `column`, `row`.
    pub fn draw(&mut self, frame: &mut Frame, column: u16, row: u16, c: char, style: CellStyle) {
        let area = self.area(column, row);
        match &mut self.face {
            Face::Bitmap {
                font,
                scale,
                glyphs,
            } => {
                let glyph = *glyphs
                    .entry(c)
                    .or_insert_with(|| font.glyph_for(c).or_else(|| font.glyph_for('?')));
                let bitmap = glyph.and_then(|glyph| font.bitmap(glyph)).unwrap_or(&[]);
                draw_bitmap(frame, area, (font.cell(), *scale), bitmap, style);
                // A pixel of the font thick.
                decor::draw(frame, area, style.decoration, style.fg, *scale);
            }
            Face::Outline(outline) => {
                draw_outline(frame, area, outline, c, style);
                // As thick as a light box line.
                let thickness = area.width / 8;
                decor::draw(frame, area, style.decoration, style.fg, thickness);
            }
        }
        frame.touch(area);
    }

    /// Draws the combining mark `mark` in `colour` over what the cell at
    /// `column`, `row` holds, centred across it; nothing when the font has
    /// no glyph for it, as a replacement character would hide the cell.
    pub fn draw_mark(&mut self, frame: &mut Frame, column: u16, row: u16, mark: char, colour: Rgb) {
        let area = self.area(column, row);
        match &mut self.face {
            // Not through the cache of `draw`, which keeps `?` for a
            // character the font lacks; marks are rare.
            Face::Bitmap { font, scale, .. } => {
                let glyph = font.glyph_for(mark);
                let Some(bitmap) = glyph.and_then(|glyph| font.bitmap(glyph)) else {
                    return;
                };
                let (width, height) = font.cell();
                let row_bytes = usize::try_from(width.div_ceil(8)).unwrap_or(0);
                for y in 0..height {
                    for x in (0..width).filter(|&x| is_lit(bitmap, row_bytes, x, y)) {
                        for (dx, dy) in
                            (0..*scale).flat_map(|dx| (0..*scale).map(move |dy| (dx, dy)))
                        {
                            frame.set(area.x + x * *scale + dx, area.y + y * *scale + dy, colour);
                        }
                    }
                }
            }
            Face::Outline(outline) => {
                let Some(glyph) = outline.glyph(mark) else {
                    return;
                };
                // Marks sit where the font puts them over the character
                // before; across the cell, they are centred on it.
                // Pixel `gx` of the glyph goes to `shift + gx` in the cell;
                // `Glyph::at` takes cell coordinates offset by `left`.
                let shift = i64::from(area.width.saturating_sub(glyph.width) / 2);
                for y in 0..area.height {
                    for x in 0..area.width {
                        let source = i64::from(x) - shift + i64::from(glyph.left);
                        let coverage =
                            u32::try_from(source).map_or(0, |source| glyph.at(source, y));
                        if coverage == 0 {
                            continue;
                        }
                        let (px, py) = (area.x + x, area.y + y);
                        let under = frame.pixel(px, py).unwrap_or(Rgb::BLACK);
                        frame.set(px, py, blend(under, colour, coverage));
                    }
                }
            }
        }
        frame.touch(area);
    }
}

/// Draws a console font's `bitmap` of a glyph `cell` pixels large, each
/// pixel `scale` times, into `area`.
fn draw_bitmap(
    frame: &mut Frame,
    area: Area,
    ((width, height), scale): ((u32, u32), u32),
    bitmap: &[u8],
    style: CellStyle,
) {
    let row_bytes = usize::try_from(width.div_ceil(8)).unwrap_or(0);
    for y in 0..height {
        for x in 0..width {
            let lit = is_lit(bitmap, row_bytes, x, y);
            let colour = if lit { style.fg } else { style.bg };
            for dy in 0..scale {
                for dx in 0..scale {
                    frame.set(area.x + x * scale + dx, area.y + y * scale + dy, colour);
                }
            }
        }
    }
}

/// The colour `alpha` (0 to 255) of the way from `bg` to `fg`.
pub fn blend(bg: Rgb, fg: Rgb, alpha: u8) -> Rgb {
    let mix = |b: u8, f: u8| {
        let (b, f, a) = (u32::from(b), u32::from(f), u32::from(alpha));
        u8::try_from((b * (255 - a) + f * a + 127) / 255).unwrap_or(u8::MAX)
    };
    Rgb(mix(bg.0, fg.0), mix(bg.1, fg.1), mix(bg.2, fg.2))
}

/// Draws `c` from `outline` into `area`: a box drawing or block character
/// as [`boxes`] draws it, else the font's glyph, else its replacement
/// character or `?`; the caller draws its decoration.
fn draw_outline(frame: &mut Frame, area: Area, outline: &mut Outline, c: char, style: CellStyle) {
    let (width, height) = (area.width, area.height);
    let mask = boxes::mask(c, width, height);
    let glyph = if mask.is_some() {
        None
    } else {
        let fallback = if outline.glyph(c).is_some() {
            c
        } else if outline.glyph('\u{fffd}').is_some() {
            '\u{fffd}'
        } else {
            '?'
        };
        // Borrowed from the cache, not copied: drawing a cell allocates
        // nothing.
        outline.glyph(fallback)
    };
    for y in 0..height {
        for x in 0..width {
            let coverage = if let Some(mask) = &mask {
                let index = u64::from(y) * u64::from(width) + u64::from(x);
                usize::try_from(index)
                    .ok()
                    .and_then(|index| mask.get(index))
                    .copied()
                    .unwrap_or(0)
            } else {
                glyph.map_or(0, |glyph| glyph.at(x, y))
            };
            frame.set(area.x + x, area.y + y, blend(style.bg, style.fg, coverage));
        }
    }
}

/// Whether the pixel at `x`, `y` of a glyph's bitmap is set.
fn is_lit(bitmap: &[u8], row_bytes: usize, x: u32, y: u32) -> bool {
    let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
        return false;
    };
    bitmap
        .get(y * row_bytes + x / 8)
        .is_some_and(|byte| byte & (0x80 >> (x % 8)) != 0)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A PSF 2 font of 8×2 glyphs, with a table: `A` with its top row set,
    /// `?` with its left column set, and `B` blank.
    fn font() -> Font {
        let mut bytes = vec![0x72, 0xb5, 0x4a, 0x86];
        for field in [0_u32, 32, 1, 3, 2, 2, 8] {
            bytes.extend_from_slice(&field.to_le_bytes());
        }
        bytes.extend_from_slice(&[0xff, 0x00, 0x80, 0x80, 0x00, 0x00]);
        bytes.extend_from_slice(b"A\xff?\xffB\xff");
        Font::parse(&bytes).unwrap()
    }

    const WHITE: Rgb = Rgb(255, 255, 255);

    fn style(underline: bool) -> CellStyle {
        CellStyle {
            fg: WHITE,
            bg: Rgb::BLACK,
            decoration: Decoration::underlined(underline),
        }
    }

    /// The rows of a frame as text: `#` for white, `.` for anything else.
    fn picture(frame: &Frame) -> Vec<String> {
        let (_, height) = frame.size();
        (0..height)
            .map(|y| {
                frame
                    .row(y)
                    .unwrap()
                    .iter()
                    .map(|&p| if p == WHITE.xrgb() { '#' } else { '.' })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn cells_are_the_font_s_scaled() {
        let grid = Grid::new(font(), 2);

        assert_eq!(grid.cell(), (16, 4));
        assert_eq!(grid.dimensions((100, 9)), (6, 2));
        let area = grid.area(1, 1);
        assert_eq!((area.x, area.y, area.width, area.height), (16, 4, 16, 4));
        assert_eq!(Grid::new(font(), 0).cell(), (8, 2));
    }

    #[test]
    fn characters_are_drawn_from_their_glyph_or_a_question_mark() {
        let mut grid = Grid::new(font(), 1);
        let mut frame = Frame::new(24, 2);

        grid.draw(&mut frame, 0, 0, 'A', style(false));
        grid.draw(&mut frame, 1, 0, 'é', style(false));
        grid.draw(&mut frame, 2, 0, 'B', style(true));

        assert_eq!(
            picture(&frame),
            ["#########.......", "........#......."]
                .iter()
                .zip(["........", "########"])
                .map(|(left, right)| format!("{left}{right}"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_doubled_font_draws_each_pixel_as_a_square() {
        let mut grid = Grid::new(font(), 2);
        let mut frame = Frame::new(16, 4);
        frame.take_damage();

        grid.draw(&mut frame, 0, 0, '?', style(false));

        assert_eq!(
            picture(&frame),
            [
                "##..............",
                "##..............",
                "##..............",
                "##.............."
            ]
        );
        assert!(frame.take_damage().is_some());
    }

    /// A grid of the image's TrueType font in cells 20 pixels high.
    fn outline_grid() -> Grid {
        let font = crate::outline::tests::DEJAVU.to_vec();
        Grid::outline(Outline::new(font, 20).unwrap()).named("DejaVuSansMono.ttf")
    }

    /// The distinct colours of the cell at `column`, 0.
    fn colours(frame: &Frame, grid: &Grid, column: u16) -> Vec<Rgb> {
        let area = grid.area(column, 0);
        let mut found = Vec::new();
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                let pixel = frame.pixel(x, y).unwrap();
                if !found.contains(&pixel) {
                    found.push(pixel);
                }
            }
        }
        found
    }

    #[test]
    fn a_truetype_font_blends_its_characters_into_the_cell() {
        let mut grid = outline_grid();
        let (width, height) = grid.cell();
        let mut frame = Frame::new(width * 4, height);

        grid.draw(&mut frame, 0, 0, 'M', style(false));
        grid.draw(&mut frame, 1, 0, '─', style(false));
        grid.draw(&mut frame, 2, 0, ' ', style(true));
        grid.draw(&mut frame, 3, 0, '\u{e000}', style(false));

        assert_eq!((grid.name(), height), ("DejaVuSansMono.ttf", 20));
        // Anti-aliased: more than the two colours of the cell.
        assert!(colours(&frame, &grid, 0).len() > 2);
        // The line crosses the whole cell, in full colour.
        let line = frame.row(height / 2).unwrap();
        assert!(
            line[usize::try_from(width).unwrap()..usize::try_from(2 * width).unwrap()]
                .iter()
                .all(|&pixel| pixel == WHITE.xrgb())
        );
        // An underline at the bottom of an empty cell.
        let bottom = frame.pixel(2 * width + 1, height - 1).unwrap();
        assert_eq!(bottom, WHITE);
        assert_eq!(frame.pixel(2 * width + 1, 0).unwrap(), Rgb::BLACK);
        // A character the font lacks shows the replacement character.
        assert!(colours(&frame, &grid, 3).len() > 1);
        assert_eq!(Grid::new(font(), 1).name(), "psf");
        assert!(format!("{grid:?}").contains("Outline"));
    }

    #[test]
    fn colours_blend_by_coverage() {
        assert_eq!(blend(Rgb::BLACK, WHITE, 0), Rgb::BLACK);
        assert_eq!(blend(Rgb::BLACK, WHITE, 255), WHITE);
        assert_eq!(
            blend(Rgb(0, 100, 200), Rgb(200, 100, 0), 128),
            Rgb(100, 100, 100)
        );
    }

    #[test]
    fn combining_marks_are_drawn_over_their_character() {
        let mut grid = outline_grid();
        let (width, height) = grid.cell();
        let mut frame = Frame::new(width * 2, height);
        grid.draw(&mut frame, 0, 0, 'e', style(false));
        let before: Vec<u32> = (0..height)
            .flat_map(|y| frame.row(y).unwrap().to_vec())
            .collect();

        grid.draw_mark(&mut frame, 0, 0, '\u{301}', WHITE);

        let changed: Vec<(u32, u32)> = (0..height)
            .flat_map(|y| (0..width * 2).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let index = usize::try_from(y * width * 2 + x).unwrap();
                frame.pixel(x, y).unwrap().xrgb() != before[index]
            })
            .collect();
        // The acute accent: above the e, inside its cell.
        assert!(!changed.is_empty());
        assert!(
            changed.iter().all(|&(x, y)| x < width && y < height / 2),
            "{changed:?}"
        );

        // A console font draws a mark it has, and nothing for one it lacks.
        let mut bitmap = Grid::new(font(), 1);
        let mut frame = Frame::new(16, 2);
        bitmap.draw(&mut frame, 0, 0, 'B', style(false));
        bitmap.draw_mark(&mut frame, 0, 0, 'A', WHITE);
        bitmap.draw_mark(&mut frame, 1, 0, '\u{301}', WHITE);
        assert_eq!(picture(&frame), ["########........", "................"]);
    }
}
