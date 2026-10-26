//! Characters from a TrueType or OpenType font (ROADMAP.md #19, step 3):
//! any character the font has, at any size, where the console fonts of the
//! kernel stop at 512 glyphs and a few fixed sizes.
//!
//! `swash` parses the font (with `skrifa`), hints its outlines to the pixel
//! grid, so that text stays sharp at console sizes, and rasterizes them as
//! coverage masks, which the grid blends between the cell's colours. Each
//! character is rasterized once.

use std::collections::HashMap;

use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::{CacheKey, FontRef};

use crate::decor::Lines;

/// A character rasterized: its coverage (0 to 255) per pixel, and where it
/// sits in the cell.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Glyph {
    /// Coverage, row by row.
    pub coverage: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Offset from the cell's left edge.
    pub left: i32,
    /// Offset from the cell's top edge.
    pub top: i32,
}

impl Glyph {
    /// The coverage at `x`, `y` of the cell; 0 outside the glyph.
    pub fn at(&self, x: u32, y: u32) -> u8 {
        self.at_signed(i64::from(x), i64::from(y))
    }

    /// The coverage at `x`, `y` from the cell's top left corner, which may
    /// lie left of it or above it; 0 outside the glyph.
    pub fn at_signed(&self, x: i64, y: i64) -> u8 {
        let (gx, gy) = (x - i64::from(self.left), y - i64::from(self.top));
        if gx < 0 || gy < 0 || gx >= i64::from(self.width) || gy >= i64::from(self.height) {
            return 0;
        }
        let index = gy * i64::from(self.width) + gx;
        usize::try_from(index)
            .ok()
            .and_then(|index| self.coverage.get(index))
            .copied()
            .unwrap_or(0)
    }
}

/// Why a font cannot be used.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutlineError {
    /// It is no TrueType or OpenType font.
    NotAFont,
    /// It has no `M`, by whose width the cells are measured.
    NoCellWidth,
}

impl std::fmt::Display for OutlineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            OutlineError::NotAFont => "not a TrueType or OpenType font",
            OutlineError::NoCellWidth => "the font has no M to measure its cells by",
        })
    }
}

impl std::error::Error for OutlineError {}

/// A font at a size: its cells and its characters.
pub struct Outline {
    data: Vec<u8>,
    offset: u32,
    key: CacheKey,
    context: ScaleContext,
    /// The size in pixels per em.
    ppem: f32,
    cell: (u32, u32),
    /// The baseline, from the cell's top edge.
    baseline: i32,
    /// Where the font puts underlines and strikeouts.
    lines: Lines,
    glyphs: HashMap<char, Option<Glyph>>,
}

impl std::fmt::Debug for Outline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Outline")
            .field("ppem", &self.ppem)
            .field("cell", &self.cell)
            .finish_non_exhaustive()
    }
}

/// The row `value` pixels below `baseline`, if it is in the cell's range.
fn row_below(baseline: i32, value: f32) -> Option<u32> {
    let rounded = value.round();
    if !(-65_536.0..65_536.0).contains(&rounded) {
        return None;
    }
    // The range check makes the conversion exact.
    #[allow(clippy::cast_possible_truncation)]
    let offset = rounded as i32;
    u32::try_from(baseline + offset).ok()
}

/// `value` rounded to a whole number of pixels, at least `least`.
fn pixels(value: f32, least: u32) -> u32 {
    let rounded = value.round();
    if (1.0..65_536.0).contains(&rounded) {
        // The range check makes the conversion exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let whole = rounded as u32;
        whole.max(least)
    } else {
        least
    }
}

impl Outline {
    /// The font in `data` (the first of a collection), sized so that a line
    /// of it, from ascent to descent, fills cells `cell_height` pixels high.
    ///
    /// # Errors
    ///
    /// When `data` is not a font, or has no `M`.
    pub fn new(data: Vec<u8>, cell_height: u32) -> Result<Outline, OutlineError> {
        let font = FontRef::from_index(&data, 0).ok_or(OutlineError::NotAFont)?;
        let (offset, key) = (font.offset, font.key);
        let metrics = font.metrics(&[]);
        let units = f32::from(metrics.units_per_em.max(1));
        let line = (metrics.ascent + metrics.descent.abs()).max(1.0);
        let height = cell_height.max(1);
        // A whole number of pixels per em hints best.
        #[allow(clippy::cast_precision_loss, reason = "cell heights are small")]
        let ppem = pixels(height as f32 * units / line, 1) as f32;
        let scaled = metrics.scale(ppem);
        let m = font.charmap().map('M');
        if m == 0 {
            return Err(OutlineError::NoCellWidth);
        }
        let width = pixels(font.glyph_metrics(&[]).scale(ppem).advance_width(m), 1);
        let line_pixels = scaled.ascent + scaled.descent.abs();
        #[allow(clippy::cast_precision_loss, reason = "cell heights are small")]
        let spare = (height as f32 - line_pixels).max(0.0) / 2.0;
        let baseline = i32::try_from(pixels(scaled.ascent + spare, 0)).unwrap_or(0);
        // The offsets go up from the baseline to the lines' tops.
        let lines = Lines {
            thickness: pixels(scaled.stroke_size, 1),
            underline: row_below(baseline, -scaled.underline_offset),
            strikeout: row_below(baseline, -scaled.strikeout_offset),
        };
        Ok(Outline {
            data,
            offset,
            key,
            context: ScaleContext::new(),
            ppem,
            cell: (width, height),
            baseline,
            lines,
            glyphs: HashMap::new(),
        })
    }

    /// The cell, as (width, height) in pixels.
    pub fn cell(&self) -> (u32, u32) {
        self.cell
    }

    /// Where the font puts underlines and strikeouts in a cell.
    pub fn lines(&self) -> Lines {
        self.lines
    }

    /// The size in pixels per em.
    pub fn ppem(&self) -> f32 {
        self.ppem
    }

    /// The glyph of `c`; `None` when the font has none.
    pub fn glyph(&mut self, c: char) -> Option<&Glyph> {
        if !self.glyphs.contains_key(&c) {
            let glyph = self.rasterize(c);
            self.glyphs.insert(c, glyph);
        }
        self.glyphs.get(&c).and_then(Option::as_ref)
    }

    /// Rasterizes `c`, placed in the cell.
    fn rasterize(&mut self, c: char) -> Option<Glyph> {
        let font = FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        };
        let id = font.charmap().map(c);
        if id == 0 {
            return None;
        }
        let mut scaler = self
            .context
            .builder(font)
            .size(self.ppem)
            .hint(true)
            .build();
        let image = Render::new(&[Source::Outline])
            .format(Format::Alpha)
            .render(&mut scaler, id)?;
        let placement = image.placement;
        Some(Glyph {
            coverage: image.data,
            width: placement.width,
            height: placement.height,
            left: placement.left,
            top: self.baseline - placement.top,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
