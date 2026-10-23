//! Box drawing and block characters drawn to fill their cell exactly, as
//! terminals draw them: a font's own glyphs for `─`, `│` or `█` rarely
//! meet their neighbours' at the cell's edges, and the menu's borders and
//! bars are made of them.
//!
//! Lines are light, heavy or double; rounded corners are drawn square and
//! dashed lines solid, as the Linux console does; diagonals are left to
//! the font.

/// How heavy a line is.
const NONE: u8 = 0;
const LIGHT: u8 = 1;
const HEAVY: u8 = 2;
const DOUBLE: u8 = 3;

/// The lines of a box character: up, down, left and right, two bits each.
const fn lines(up: u8, down: u8, left: u8, right: u8) -> u8 {
    (up << 6) | (down << 4) | (left << 2) | right
}

/// The lines of U+2500 to U+257F, in order; 0 for those left to the font.
#[rustfmt::skip]
const BOXES: [u8; 128] = [
    // ─ ━ │ ┃ ┄ ┅ ┆ ┇
    lines(0, 0, 1, 1), lines(0, 0, 2, 2), lines(1, 1, 0, 0), lines(2, 2, 0, 0),
    lines(0, 0, 1, 1), lines(0, 0, 2, 2), lines(1, 1, 0, 0), lines(2, 2, 0, 0),
    // ┈ ┉ ┊ ┋ ┌ ┍ ┎ ┏
    lines(0, 0, 1, 1), lines(0, 0, 2, 2), lines(1, 1, 0, 0), lines(2, 2, 0, 0),
    lines(0, 1, 0, 1), lines(0, 1, 0, 2), lines(0, 2, 0, 1), lines(0, 2, 0, 2),
    // ┐ ┑ ┒ ┓ └ ┕ ┖ ┗
    lines(0, 1, 1, 0), lines(0, 1, 2, 0), lines(0, 2, 1, 0), lines(0, 2, 2, 0),
    lines(1, 0, 0, 1), lines(1, 0, 0, 2), lines(2, 0, 0, 1), lines(2, 0, 0, 2),
    // ┘ ┙ ┚ ┛ ├ ┝ ┞ ┟
    lines(1, 0, 1, 0), lines(1, 0, 2, 0), lines(2, 0, 1, 0), lines(2, 0, 2, 0),
    lines(1, 1, 0, 1), lines(1, 1, 0, 2), lines(2, 1, 0, 1), lines(1, 2, 0, 1),
    // ┠ ┡ ┢ ┣ ┤ ┥ ┦ ┧
    lines(2, 2, 0, 1), lines(2, 1, 0, 2), lines(1, 2, 0, 2), lines(2, 2, 0, 2),
    lines(1, 1, 1, 0), lines(1, 1, 2, 0), lines(2, 1, 1, 0), lines(1, 2, 1, 0),
    // ┨ ┩ ┪ ┫ ┬ ┭ ┮ ┯
    lines(2, 2, 1, 0), lines(2, 1, 2, 0), lines(1, 2, 2, 0), lines(2, 2, 2, 0),
    lines(0, 1, 1, 1), lines(0, 1, 2, 1), lines(0, 1, 1, 2), lines(0, 1, 2, 2),
    // ┰ ┱ ┲ ┳ ┴ ┵ ┶ ┷
    lines(0, 2, 1, 1), lines(0, 2, 2, 1), lines(0, 2, 1, 2), lines(0, 2, 2, 2),
    lines(1, 0, 1, 1), lines(1, 0, 2, 1), lines(1, 0, 1, 2), lines(1, 0, 2, 2),
    // ┸ ┹ ┺ ┻ ┼ ┽ ┾ ┿
    lines(2, 0, 1, 1), lines(2, 0, 2, 1), lines(2, 0, 1, 2), lines(2, 0, 2, 2),
    lines(1, 1, 1, 1), lines(1, 1, 2, 1), lines(1, 1, 1, 2), lines(1, 1, 2, 2),
    // ╀ ╁ ╂ ╃ ╄ ╅ ╆ ╇
    lines(2, 1, 1, 1), lines(1, 2, 1, 1), lines(2, 2, 1, 1), lines(2, 1, 2, 1),
    lines(2, 1, 1, 2), lines(1, 2, 2, 1), lines(1, 2, 1, 2), lines(2, 1, 2, 2),
    // ╈ ╉ ╊ ╋ ╌ ╍ ╎ ╏
    lines(1, 2, 2, 2), lines(2, 2, 2, 1), lines(2, 2, 1, 2), lines(2, 2, 2, 2),
    lines(0, 0, 1, 1), lines(0, 0, 2, 2), lines(1, 1, 0, 0), lines(2, 2, 0, 0),
    // ═ ║ ╒ ╓ ╔ ╕ ╖ ╗
    lines(0, 0, 3, 3), lines(3, 3, 0, 0), lines(0, 1, 0, 3), lines(0, 3, 0, 1),
    lines(0, 3, 0, 3), lines(0, 1, 3, 0), lines(0, 3, 1, 0), lines(0, 3, 3, 0),
    // ╘ ╙ ╚ ╛ ╜ ╝ ╞ ╟
    lines(1, 0, 0, 3), lines(3, 0, 0, 1), lines(3, 0, 0, 3), lines(1, 0, 3, 0),
    lines(3, 0, 1, 0), lines(3, 0, 3, 0), lines(1, 1, 0, 3), lines(3, 3, 0, 1),
    // ╠ ╡ ╢ ╣ ╤ ╥ ╦ ╧
    lines(3, 3, 0, 3), lines(1, 1, 3, 0), lines(3, 3, 1, 0), lines(3, 3, 3, 0),
    lines(0, 1, 3, 3), lines(0, 3, 1, 1), lines(0, 3, 3, 3), lines(1, 0, 3, 3),
    // ╨ ╩ ╪ ╫ ╬ ╭ ╮ ╯
    lines(3, 0, 1, 1), lines(3, 0, 3, 3), lines(1, 1, 3, 3), lines(3, 3, 1, 1),
    lines(3, 3, 3, 3), lines(0, 1, 0, 1), lines(0, 1, 1, 0), lines(1, 0, 1, 0),
    // ╰ ╱ ╲ ╳ ╴ ╵ ╶ ╷
    lines(1, 0, 0, 1), 0, 0, 0,
    lines(0, 0, 1, 0), lines(1, 0, 0, 0), lines(0, 0, 0, 1), lines(0, 1, 0, 0),
    // ╸ ╹ ╺ ╻ ╼ ╽ ╾ ╿
    lines(0, 0, 2, 0), lines(2, 0, 0, 0), lines(0, 0, 0, 2), lines(0, 2, 0, 0),
    lines(0, 0, 1, 2), lines(1, 2, 0, 0), lines(0, 0, 2, 1), lines(2, 1, 0, 0),
];

/// A coverage mask of a cell, 0 to 255 per pixel.
struct Mask {
    width: u32,
    height: u32,
    coverage: Vec<u8>,
}

impl Mask {
    fn new(width: u32, height: u32) -> Mask {
        let size = usize::try_from(u64::from(width) * u64::from(height)).unwrap_or(0);
        Mask {
            width,
            height,
            coverage: vec![0; size],
        }
    }

    /// Sets the coverage of the pixels from `x0` to `x1` and `y0` to `y1`
    /// (excluded), clipped to the cell.
    fn fill(&mut self, x: (u32, u32), y: (u32, u32), value: u8) {
        for row in y.0..y.1.min(self.height) {
            for column in x.0..x.1.min(self.width) {
                let index = u64::from(row) * u64::from(self.width) + u64::from(column);
                if let Some(pixel) = usize::try_from(index)
                    .ok()
                    .and_then(|index| self.coverage.get_mut(index))
                {
                    *pixel = value;
                }
            }
        }
    }
}

/// The bands, as (start, thickness), that a line of `weight` drawn across
/// at `centre` takes, with light lines `light` pixels thick.
fn bands(weight: u8, centre: u32, light: u32) -> Vec<(u32, u32)> {
    match weight {
        LIGHT => vec![(centre.saturating_sub(light / 2), light)],
        HEAVY => vec![(centre.saturating_sub(light), 2 * light)],
        DOUBLE => {
            let first = centre.saturating_sub(3 * light / 2);
            vec![(first, light), (first + 2 * light, light)]
        }
        _ => Vec::new(),
    }
}

/// The pixels, as (first, past the last), that the lines of `weights`
/// drawn across at `centre` cover together; `None` without lines.
fn span(weights: [u8; 2], centre: u32, light: u32) -> Option<(u32, u32)> {
    let bands: Vec<(u32, u32)> = weights
        .iter()
        .flat_map(|&weight| bands(weight, centre, light))
        .collect();
    let first = bands.iter().map(|&(start, _)| start).min()?;
    let last = bands
        .iter()
        .map(|&(start, thickness)| start + thickness)
        .max()?;
    Some((first, last))
}

/// The pixels along its length that a half line of `weight` covers: from
/// the cell's edge (`towards_start`: its first pixel) to the far side of
/// the lines across it, `across`, or, without any, of a line of its own
/// weight across it.
fn length(
    weight: u8,
    across: Option<(u32, u32)>,
    centre: u32,
    size: u32,
    light: u32,
    towards_start: bool,
) -> (u32, u32) {
    let (first, last) = across
        .or_else(|| span([weight, NONE], centre, light))
        .unwrap_or((centre, centre));
    if towards_start {
        (0, last)
    } else {
        (first, size)
    }
}

/// Draws the lines `code` (as [`lines`] packs them) into `mask`.
fn draw_lines(mask: &mut Mask, code: u8) {
    let [up, down, left, right] = [code >> 6, (code >> 4) & 3, (code >> 2) & 3, code & 3];
    let (width, height) = (mask.width, mask.height);
    let light = (width / 8).max(1);
    let (cx, cy) = (width / 2, height / 2);
    // Each half line reaches across the lines that cross it, so that
    // corners and joins close.
    let vertical = span([up, down], cx, light);
    let horizontal = span([left, right], cy, light);
    for (weight, towards_start) in [(left, true), (right, false)] {
        let (x0, x1) = length(weight, vertical, cx, width, light, towards_start);
        for (start, thickness) in bands(weight, cy, light) {
            mask.fill((x0, x1), (start, start + thickness), 255);
        }
    }
    for (weight, towards_start) in [(up, true), (down, false)] {
        let (y0, y1) = length(weight, horizontal, cy, height, light, towards_start);
        for (start, thickness) in bands(weight, cx, light) {
            mask.fill((start, start + thickness), (y0, y1), 255);
        }
    }
}

/// `part` eighths of `whole`, at least one pixel when `part` is not 0.
fn eighths(whole: u32, part: u32) -> u32 {
    if part == 0 {
        0
    } else {
        (whole * part / 8).max(1)
    }
}

/// Draws the block element `c` (U+2580 to U+259F) into `mask`.
fn draw_block(mask: &mut Mask, c: u32) {
    let (w, h) = (mask.width, mask.height);
    match c {
        0x2580 => mask.fill((0, w), (0, h / 2), 255),
        0x2581..=0x2588 => mask.fill((0, w), (h - eighths(h, c - 0x2580), h), 255),
        0x2589..=0x258f => mask.fill((0, eighths(w, 0x2590 - c)), (0, h), 255),
        0x2590 => mask.fill((w / 2, w), (0, h), 255),
        0x2591..=0x2593 => {
            let shade = u8::try_from((c - 0x2590) * 64).unwrap_or(u8::MAX);
            mask.fill((0, w), (0, h), shade);
        }
        0x2594 => mask.fill((0, w), (0, eighths(h, 1)), 255),
        0x2595 => mask.fill((w - eighths(w, 1), w), (0, h), 255),
        _ => {
            // Quadrants: upper left 1, upper right 2, lower left 4, lower
            // right 8.
            let quadrants = [
                4,
                8,
                1,
                1 | 4 | 8,
                1 | 8,
                1 | 2 | 4,
                1 | 2 | 8,
                2,
                2 | 4,
                2 | 4 | 8,
            ];
            let index = usize::try_from(c.saturating_sub(0x2596)).unwrap_or(usize::MAX);
            let bits = quadrants.get(index).copied().unwrap_or(0);
            let (mx, my) = (w / 2, h / 2);
            for (bit, x, y) in [
                (1, (0, mx), (0, my)),
                (2, (mx, w), (0, my)),
                (4, (0, mx), (my, h)),
                (8, (mx, w), (my, h)),
            ] {
                if bits & bit != 0 {
                    mask.fill(x, y, 255);
                }
            }
        }
    }
}

/// The coverage of `c` in a cell of `width` × `height` pixels, row by row,
/// when it is a box drawing or block character this module draws.
pub fn mask(c: char, width: u32, height: u32) -> Option<Vec<u8>> {
    let code = u32::from(c);
    let mut mask = Mask::new(width, height);
    match code {
        0x2500..=0x257f => {
            let lines = BOXES.get(usize::try_from(code - 0x2500).ok()?).copied()?;
            if lines == 0 {
                return None;
            }
            draw_lines(&mut mask, lines);
        }
        0x2580..=0x259f => draw_block(&mut mask, code),
        _ => return None,
    }
    Some(mask.coverage)
}

#[cfg(test)]
mod tests;
