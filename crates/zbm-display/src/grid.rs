//! Text on the frame: a grid of character cells drawn with a console font,
//! the image's Terminus or Spleen fonts that the kernel console uses too,
//! at the size `zbm console-autosize` chooses, doubled where it says so.

use std::collections::HashMap;

use zbm_types::psf::Font;

use crate::frame::{Area, Frame, Rgb};

/// How a cell is drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellStyle {
    /// The colour of the character.
    pub fg: Rgb,
    /// The colour around it.
    pub bg: Rgb,
    /// Whether a line is drawn under it.
    pub underline: bool,
}

/// A font at a scale, which draws characters into cells of a frame.
#[derive(Clone, Debug)]
pub struct Grid {
    font: Font,
    scale: u32,
    /// The glyph of each character drawn so far: finding one searches the
    /// font's whole table.
    glyphs: HashMap<char, Option<usize>>,
}

impl Grid {
    /// Draws with `font`, each of its pixels `scale` pixels wide and high
    /// (2 for a font doubled as `setfont -d` doubles it).
    pub fn new(font: Font, scale: u32) -> Grid {
        Grid {
            font,
            scale: scale.max(1),
            glyphs: HashMap::new(),
        }
    }

    /// The cell on the frame, as (width, height) in pixels.
    pub fn cell(&self) -> (u32, u32) {
        let (width, height) = self.font.cell();
        (width * self.scale, height * self.scale)
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

    /// The glyph that draws `c`: its own, a look-alike, or else `?`.
    fn glyph(&mut self, c: char) -> Option<usize> {
        let font = &self.font;
        *self
            .glyphs
            .entry(c)
            .or_insert_with(|| font.glyph_for(c).or_else(|| font.glyph_for('?')))
    }

    /// Draws `c` in the cell at `column`, `row`.
    pub fn draw(&mut self, frame: &mut Frame, column: u16, row: u16, c: char, style: CellStyle) {
        let area = self.area(column, row);
        let glyph = self.glyph(c);
        let bitmap = glyph
            .and_then(|glyph| self.font.bitmap(glyph))
            .unwrap_or(&[]);
        let (width, height) = self.font.cell();
        let row_bytes = usize::try_from(width.div_ceil(8)).unwrap_or(0);
        for y in 0..height {
            let underline = style.underline && y + 1 == height;
            for x in 0..width {
                let lit = underline || is_lit(bitmap, row_bytes, x, y);
                let colour = if lit { style.fg } else { style.bg };
                for dy in 0..self.scale {
                    for dx in 0..self.scale {
                        let px = area.x + x * self.scale + dx;
                        frame.set(px, area.y + y * self.scale + dy, colour);
                    }
                }
            }
        }
        frame.touch(area);
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
            underline,
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
}
