//! The characters drawn as shapes rather than taken from the font, besides
//! the lines and blocks of [`super`]: Braille patterns (U+2800 to U+28FF),
//! the box drawing diagonals (U+2571 to U+2573), the Powerline triangles
//! and arrows (U+E0B0 to U+E0B3, U+E0B8 to U+E0BF) and the sextants of
//! Symbols for Legacy Computing (U+1FB00 to U+1FB3B). Fonts rarely have
//! them, and when they do, their shapes do not fill the cell, so that
//! prompts and graphs made of them break at the cells' edges.
//!
//! Each pixel is sampled at 4 × 4 points, in eighths of a pixel and whole
//! numbers, so that edges are smooth.

use super::Mask;

/// How many eighths of a pixel there are in a pixel.
const SUB: i64 = 8;

/// Sets each pixel of `mask` to the share of its 16 sample points that
/// `inside` takes, which gets them in eighths of a pixel from the cell's
/// top left corner.
fn paint(mask: &mut Mask, inside: impl Fn(i64, i64) -> bool) {
    for y in 0..mask.height {
        for x in 0..mask.width {
            let (px, py) = (i64::from(x) * SUB, i64::from(y) * SUB);
            let hits = (0..4)
                .flat_map(|i| (0..4).map(move |j| (i, j)))
                .filter(|&(i, j)| inside(px + 2 * i + 1, py + 2 * j + 1))
                .count();
            if hits > 0 {
                let value = u8::try_from(hits * 255 / 16).unwrap_or(u8::MAX);
                mask.fill((x, x + 1), (y, y + 1), value);
            }
        }
    }
}

/// The cell's size in eighths of a pixel.
fn size(mask: &Mask) -> (i64, i64) {
    (i64::from(mask.width) * SUB, i64::from(mask.height) * SUB)
}

/// Whether the point `p` is within `half` of the line through `a` and
/// `b` (all in eighths of a pixel).
fn is_near_line(p: (i64, i64), a: (i64, i64), b: (i64, i64), half: i64) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let cross = dx * (p.1 - a.1) - dy * (p.0 - a.0);
    cross * cross <= half * half * (dx * dx + dy * dy)
}

/// Whether the point `p` is on the side of the line from `a` to `b` that
/// `side` (1 or -1) names, or on it.
fn is_on_side(p: (i64, i64), a: (i64, i64), b: (i64, i64), side: i64) -> bool {
    let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    cross * side >= 0
}

/// Half the thickness of a light line, in eighths of a pixel: a line is an
/// eighth of the cell's width, as in [`super`].
fn half_line(mask: &Mask) -> i64 {
    (i64::from((mask.width / 8).max(1)) * SUB) / 2
}

/// Draws the diagonals `rising` (from the bottom left to the top right)
/// and `falling`, corner to corner.
fn draw_diagonals(mask: &mut Mask, rising: bool, falling: bool) {
    let (w, h) = size(mask);
    let half = half_line(mask);
    paint(mask, |x, y| {
        (rising && is_near_line((x, y), (0, h), (w, 0), half))
            || (falling && is_near_line((x, y), (0, 0), (w, h), half))
    });
}

/// Draws the Braille pattern of `dots`: dots 1 to 3 and 7 down the left
/// column, 4 to 6 and 8 down the right, bit 0 for dot 1.
fn draw_braille(mask: &mut Mask, dots: u8) {
    let (w, h) = size(mask);
    let radius = (w / 4).min(h / 8) * 3 / 4;
    let centre = |column: i64, row: i64| (w * (2 * column + 1) / 4, h * (2 * row + 1) / 8);
    let places = [
        (0, 0),
        (0, 1),
        (0, 2),
        (1, 0),
        (1, 1),
        (1, 2),
        (0, 3),
        (1, 3),
    ];
    let centres: Vec<(i64, i64)> = places
        .iter()
        .enumerate()
        .filter(|&(bit, _)| dots & (1 << bit) != 0)
        .map(|(_, &(column, row))| centre(column, row))
        .collect();
    paint(mask, |x, y| {
        centres.iter().any(|&(cx, cy)| {
            let (dx, dy) = (x - cx, y - cy);
            dx * dx + dy * dy <= radius * radius
        })
    });
}

/// Draws the sextant of `bits`: 1 and 2 the top row's left and right
/// thirds, 4 and 8 the middle row's, 16 and 32 the bottom row's.
fn draw_sextant(mask: &mut Mask, bits: u8) {
    let (w, h) = (mask.width, mask.height);
    let rows = [0, h / 3, 2 * h / 3, h];
    for (row, edges) in rows.windows(2).enumerate() {
        let (&[top, bottom], Ok(row)) = (edges, u8::try_from(row)) else {
            continue;
        };
        for (column, x) in [(0, (0, w / 2)), (1, (w / 2, w))] {
            if bits & (1 << (2 * row + column)) != 0 {
                mask.fill(x, (top, bottom), 255);
            }
        }
    }
}

/// The sextant bits of U+1FB00 plus `index`: every pattern but none, all,
/// and the left and right halves, which are older characters (U+258C,
/// U+2590).
fn sextant_bits(index: u32) -> Option<u8> {
    let mut bits = u8::try_from(index).ok()?.checked_add(1)?;
    for skipped in [21, 42] {
        if bits >= skipped {
            bits += 1;
        }
    }
    (bits < 63).then_some(bits)
}

/// Draws the Powerline character `c` (U+E0B0 to U+E0B3, U+E0B8 to U+E0BF).
fn draw_powerline(mask: &mut Mask, c: u32) -> Option<()> {
    let (w, h) = size(mask);
    let half = half_line(mask);
    let (top_left, top_right, bottom_left, bottom_right) = ((0, 0), (w, 0), (0, h), (w, h));
    let (left_middle, right_middle) = ((0, h / 2), (w, h / 2));
    match c {
        // ▶ and ◀, the cell's height tall.
        0xe0b0 => paint(mask, |x, y| {
            is_on_side((x, y), top_left, right_middle, 1)
                && is_on_side((x, y), bottom_left, right_middle, -1)
        }),
        0xe0b2 => paint(mask, |x, y| {
            is_on_side((x, y), top_right, left_middle, -1)
                && is_on_side((x, y), bottom_right, left_middle, 1)
        }),
        // ❯ and ❮.
        0xe0b1 => paint(mask, |x, y| {
            is_near_line((x, y), top_left, right_middle, half) && y <= h / 2
                || is_near_line((x, y), bottom_left, right_middle, half) && y >= h / 2
        }),
        0xe0b3 => paint(mask, |x, y| {
            is_near_line((x, y), top_right, left_middle, half) && y <= h / 2
                || is_near_line((x, y), bottom_right, left_middle, half) && y >= h / 2
        }),
        // Triangles in the corners: lower left, lower right, upper left,
        // upper right; their odd neighbours are the diagonal alone.
        0xe0b8 => paint(mask, |x, y| is_on_side((x, y), top_left, bottom_right, 1)),
        0xe0ba => paint(mask, |x, y| is_on_side((x, y), bottom_left, top_right, 1)),
        0xe0bc => paint(mask, |x, y| is_on_side((x, y), bottom_left, top_right, -1)),
        0xe0be => paint(mask, |x, y| is_on_side((x, y), top_left, bottom_right, -1)),
        0xe0b9 | 0xe0bf => draw_diagonals(mask, false, true),
        0xe0bb | 0xe0bd => draw_diagonals(mask, true, false),
        _ => return None,
    }
    Some(())
}

/// Draws `c` into `mask` when it is one of this module's characters.
pub(super) fn draw(mask: &mut Mask, c: u32) -> Option<()> {
    match c {
        0x2571 => draw_diagonals(mask, true, false),
        0x2572 => draw_diagonals(mask, false, true),
        0x2573 => draw_diagonals(mask, true, true),
        0x2800..=0x28ff => draw_braille(mask, u8::try_from(c - 0x2800).ok()?),
        0x1fb00..=0x1fb3b => draw_sextant(mask, sextant_bits(c - 0x1fb00)?),
        _ => draw_powerline(mask, c)?,
    }
    Some(())
}

#[cfg(test)]
mod tests;
