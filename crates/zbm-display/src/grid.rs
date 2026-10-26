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
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CellStyle {
    /// The colour of the character.
    pub fg: Rgb,
    /// The colour around it.
    pub bg: Rgb,
    /// The lines drawn under or through it.
    pub decoration: Decoration,
}

/// What is drawn over a span of cells: a character and its style, over
/// two cells for a wide one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Text {
    /// The character.
    pub c: char,
    /// How it is drawn.
    pub style: CellStyle,
    /// How many cells it takes: 1, or 2 for a wide character.
    pub span: u16,
}

/// A character next to the one drawn, whose glyph may reach into its
/// cells, as italic and some wide letters do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Neighbour {
    /// The character.
    pub c: char,
    /// Its colour.
    pub fg: Rgb,
    /// Where its cell starts, in pixels from the left edge of the cells
    /// drawn: negative on the left.
    pub offset: i64,
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
        let text = Text { c, style, span: 1 };
        self.draw_text(frame, column, row, text, &[]);
    }

    /// Draws `text` in the cells from `column`, `row`, with the parts of
    /// the glyphs of `neighbours` that reach into them over it.
    pub fn draw_text(
        &mut self,
        frame: &mut Frame,
        column: u16,
        row: u16,
        text: Text,
        neighbours: &[Neighbour],
    ) {
        let cell = self.area(column, row);
        let area = Area {
            width: cell.width * u32::from(text.span.max(1)),
            ..cell
        };
        let Text { c, style, .. } = text;
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
                // A console font's glyph is one cell wide: centred across
                // the span.
                frame.fill(area, style.bg);
                let centred = Area {
                    x: area.x + (area.width - cell.width) / 2,
                    ..cell
                };
                draw_bitmap(frame, centred, (font.cell(), *scale), bitmap, style);
                // A pixel of the font thick.
                let lines = decor::Lines::thick(*scale);
                decor::draw(frame, area, style.decoration, style.fg, lines);
            }
            Face::Outline(outline) => {
                draw_outline(frame, area, outline, c, style);
                for &neighbour in neighbours {
                    draw_overhang(frame, area, outline, neighbour);
                }
                let lines = outline.lines();
                decor::draw(frame, area, style.decoration, style.fg, lines);
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

/// Blends into `area` the part of the glyph of `neighbour` that reaches
/// into it. Shapes that [`boxes`] draws stay in their cells.
fn draw_overhang(frame: &mut Frame, area: Area, outline: &mut Outline, neighbour: Neighbour) {
    if boxes::is_drawn(neighbour.c) {
        return;
    }
    let Some(glyph) = outline.glyph(neighbour.c) else {
        return;
    };
    let from = neighbour.offset + i64::from(glyph.left);
    let to = from + i64::from(glyph.width);
    let (start, end) = (from.max(0), to.min(i64::from(area.width)));
    for y in 0..area.height {
        for x in start..end {
            let coverage = glyph.at_signed(x - neighbour.offset, i64::from(y));
            let Ok(x) = u32::try_from(x) else {
                continue;
            };
            if coverage == 0 {
                continue;
            }
            let (px, py) = (area.x + x, area.y + y);
            let under = frame.pixel(px, py).unwrap_or(Rgb::BLACK);
            frame.set(px, py, blend(under, neighbour.fg, coverage));
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
mod tests;
