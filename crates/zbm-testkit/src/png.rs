//! PNG files of drawn frames, for the snapshot tests that compare what a
//! console or the menu draws with reference images in the repository:
//! `zbm_display::png`, with rows of pixels.

use zbm_display::frame::Rgb;

/// The PNG file of an image `width` pixels wide whose rows are `rows`,
/// each pixel as (red, green, blue).
///
/// # Panics
///
/// When a row is not `width` pixels wide.
pub fn png(width: u32, rows: &[Vec<(u8, u8, u8)>]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(rows.len() * width as usize);
    for row in rows {
        assert_eq!(row.len(), width as usize, "every row is the image's width");
        pixels.extend(row.iter().map(|&(r, g, b)| Rgb(r, g, b)));
    }
    zbm_display::png::png(width, &pixels)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn rows_are_the_display_s_pixels() {
        let rows = vec![vec![(255, 0, 0), (0, 255, 0)], vec![(0, 0, 255), (1, 2, 3)]];
        let pixels = [Rgb(255, 0, 0), Rgb(0, 255, 0), Rgb(0, 0, 255), Rgb(1, 2, 3)];
        assert_eq!(png(2, &rows), zbm_display::png::png(2, &pixels));
    }

    #[test]
    #[should_panic(expected = "every row is the image's width")]
    fn rows_of_another_width_are_refused() {
        png(3, &[vec![(0, 0, 0)]]);
    }
}
