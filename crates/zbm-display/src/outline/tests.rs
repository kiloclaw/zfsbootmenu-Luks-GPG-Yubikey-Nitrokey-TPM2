//! The image's TrueType font: its cells at the size asked for, and its
//! characters placed in them.

use pretty_assertions::assert_eq;

use super::*;

/// The TrueType font that images with the Rust runtime carry.
pub(crate) const DEJAVU: &[u8] =
    include_bytes!("../../../../legacy/zfsbootmenu/fonts/dejavu/DejaVuSansMono.ttf");

/// The pixels a glyph covers inside the cell.
fn inked(outline: &mut Outline, c: char) -> u32 {
    let (width, height) = outline.cell();
    let glyph = outline.glyph(c).unwrap().clone();
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| glyph.at(x, y) > 0)
        .map(|_| 1)
        .sum()
}

#[test]
fn cells_have_the_height_asked_for_and_the_font_s_width() {
    for (height, width) in [(16, 8), (20, 10), (32, 16), (64, 32)] {
        let outline = Outline::new(DEJAVU.to_vec(), height).unwrap();
        let (w, h) = outline.cell();
        assert_eq!(h, height);
        assert!(w.abs_diff(width) <= 1, "{height}: {w}");
        assert!(outline.ppem() > 0.0);
    }
    assert!(format!("{:?}", Outline::new(DEJAVU.to_vec(), 20).unwrap()).contains("cell"));
}

#[test]
fn characters_of_many_scripts_are_drawn_inside_their_cell() {
    let mut outline = Outline::new(DEJAVU.to_vec(), 32).unwrap();
    let (width, height) = outline.cell();

    for c in ['M', 'g', 'É', 'œ', 'ß', 'Ж', 'λ', '▶', '…'] {
        let glyph = outline.glyph(c).unwrap().clone();
        assert!(glyph.width > 0 && glyph.height > 0, "{c}");
        assert!(glyph.left >= -1 && glyph.top >= -1, "{c}: {glyph:?}");
        assert!(
            glyph.left + i32::try_from(glyph.width).unwrap() <= i32::try_from(width).unwrap() + 2,
            "{c}: {glyph:?}"
        );
        assert!(
            glyph.top + i32::try_from(glyph.height).unwrap() <= i32::try_from(height).unwrap() + 1,
            "{c}: {glyph:?}"
        );
        assert!(inked(&mut outline, c) > 10, "{c}");
    }
    // A space has nothing to draw.
    assert_eq!(inked(&mut outline, ' '), 0);
    // The descender of g reaches below the baseline of M.
    let g = outline.glyph('g').unwrap().clone();
    let m = outline.glyph('M').unwrap().clone();
    assert!(g.top + i32::try_from(g.height).unwrap() > m.top + i32::try_from(m.height).unwrap());
}

#[test]
fn characters_the_font_lacks_and_files_that_are_no_fonts() {
    let mut outline = Outline::new(DEJAVU.to_vec(), 20).unwrap();

    assert_eq!(outline.glyph('\u{e000}'), None);
    assert_eq!(outline.glyph('\u{e000}'), None);
    let error = Outline::new(b"not a font".to_vec(), 20).unwrap_err();
    assert_eq!(error, OutlineError::NotAFont);
    assert_eq!(error.to_string(), "not a TrueType or OpenType font");
    assert!(OutlineError::NoCellWidth.to_string().contains('M'));
}

#[test]
fn coverage_outside_a_glyph_is_nothing() {
    let glyph = Glyph {
        coverage: vec![10, 20, 30, 40],
        width: 2,
        height: 2,
        left: 1,
        top: 1,
    };

    assert_eq!(glyph.at(1, 1), 10);
    assert_eq!(glyph.at(2, 2), 40);
    assert_eq!(glyph.at(0, 1), 0);
    assert_eq!(glyph.at(3, 1), 0);
    assert_eq!(glyph.at(1, 3), 0);
    assert_eq!(pixels(f32::NAN, 3), 3);
    assert_eq!(pixels(0.2, 1), 1);
    assert_eq!(pixels(7.6, 1), 8);
}

#[test]
fn the_font_places_its_underline_and_strikeout() {
    let outline = Outline::new(DEJAVU.to_vec(), 20).unwrap();
    let lines = outline.lines();
    let (underline, strikeout) = (lines.underline.unwrap(), lines.strikeout.unwrap());

    assert!(lines.thickness >= 1);
    // Below the middle and above the bottom; the strikeout across the
    // middle of lower-case letters, above the underline.
    assert!(underline > 10 && underline < 20, "{lines:?}");
    assert!(strikeout > 5 && strikeout < underline, "{lines:?}");
}
