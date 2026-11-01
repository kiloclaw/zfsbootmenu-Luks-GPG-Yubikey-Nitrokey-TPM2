//! A console font as the kernel console takes it (ROADMAP.md CD5): its
//! glyphs one after the other, doubled when asked as `setfont -d` doubles
//! them, and the characters of the Basic Multilingual Plane each draws.

use zbm_sys::font::KernelFont;

use super::{Font, row_bytes};

impl Font {
    /// The font for the kernel console, each pixel doubled across and down
    /// when `doubled`.
    pub fn kernel_font(&self, doubled: bool) -> KernelFont {
        let scale = if doubled { 2 } else { 1 };
        let width = self.width * scale;
        let height = self.height * scale;
        let glyphs = self
            .glyphs
            .iter()
            .flat_map(|glyph| {
                if doubled {
                    double(glyph, self.width, self.height)
                } else {
                    glyph.clone()
                }
            })
            .collect();
        let map = self
            .chars
            .iter()
            .enumerate()
            .flat_map(|(index, chars)| {
                let index = u16::try_from(index).ok();
                chars
                    .iter()
                    .filter_map(move |&c| Some((u16::try_from(u32::from(c)).ok()?, index?)))
            })
            .collect();
        KernelFont {
            width,
            height,
            count: u32::try_from(self.glyphs.len()).unwrap_or(u32::MAX),
            glyphs,
            map,
        }
    }
}

/// `glyph`, of `width` by `height` pixels, with each pixel doubled across
/// and down.
fn double(glyph: &[u8], width: u32, height: u32) -> Vec<u8> {
    let row = usize::try_from(row_bytes(width)).unwrap_or(usize::MAX);
    let wide = usize::try_from(row_bytes(width * 2)).unwrap_or(usize::MAX);
    let mut doubled = Vec::with_capacity(wide * 2 * usize::try_from(height).unwrap_or(0));
    for line in glyph.chunks(row) {
        let mut out = vec![0_u8; wide];
        for x in 0..width {
            let byte = line.get(usize::try_from(x / 8).unwrap_or(usize::MAX));
            if byte.is_some_and(|byte| byte & (0x80 >> (x % 8)) != 0) {
                for dx in [x * 2, x * 2 + 1] {
                    if let Some(byte) = out.get_mut(usize::try_from(dx / 8).unwrap_or(usize::MAX)) {
                        *byte |= 0x80 >> (dx % 8);
                    }
                }
            }
        }
        doubled.extend_from_slice(&out);
        doubled.extend_from_slice(&out);
    }
    doubled
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A font of two 3x2 glyphs: `A` and `é` and a character beyond the
    /// Basic Multilingual Plane on the first, nothing on the second.
    fn font() -> Font {
        Font {
            width: 3,
            height: 2,
            glyphs: vec![vec![0b1010_0000, 0b0100_0000], vec![0, 0b1110_0000]],
            chars: vec![vec!['A', 'é', '😀'], Vec::new()],
        }
    }

    #[test]
    fn the_kernel_s_font_is_the_glyphs_in_order_and_the_bmp_characters() {
        let kernel = font().kernel_font(false);
        assert_eq!((kernel.width, kernel.height, kernel.count), (3, 2, 2));
        assert_eq!(kernel.glyphs, [0b1010_0000, 0b0100_0000, 0, 0b1110_0000]);
        assert_eq!(kernel.map, [(0x41, 0), (0xe9, 0)]);
    }

    #[test]
    fn a_doubled_font_has_each_pixel_twice_across_and_down() {
        let kernel = font().kernel_font(true);
        assert_eq!((kernel.width, kernel.height, kernel.count), (6, 4, 2));
        let first: Vec<u8> = kernel.glyphs[..4].to_vec();
        assert_eq!(first, [0b1100_1100, 0b1100_1100, 0b0011_0000, 0b0011_0000]);
        // A glyph 5 wide doubles to 10 pixels: two bytes a row.
        assert_eq!(
            double(&[0b1000_1000], 5, 1),
            [0b1100_0000, 0b1100_0000, 0b1100_0000, 0b1100_0000]
        );
    }
}
