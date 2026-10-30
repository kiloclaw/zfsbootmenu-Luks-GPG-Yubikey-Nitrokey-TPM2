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
        face: FontFace::Regular,
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
    assert!(!frame.take_damage().is_empty());
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
    // An underline in an empty cell, where the font puts it: below the
    // middle, above the bottom row.
    let lit: Vec<u32> = (0..height)
        .filter(|&y| frame.pixel(2 * width + 1, y) == Some(WHITE))
        .collect();
    assert!(
        !lit.is_empty() && lit.iter().all(|&y| y > height / 2 && y < height - 1),
        "{lit:?}"
    );
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

#[test]
fn a_wide_character_takes_two_cells() {
    let red = Rgb(255, 0, 0);
    let wide = CellStyle {
        bg: red,
        ..style(false)
    };
    let text = Text {
        c: 'A',
        style: wide,
        span: 2,
    };
    // The console font's glyph, one cell wide, in the middle of the two.
    let mut grid = Grid::new(font(), 1);
    let mut frame = Frame::new(24, 2);
    grid.draw_text(&mut frame, 0, 0, text, &[]);
    let row: Vec<Option<Rgb>> = (0..24).map(|x| frame.pixel(x, 1)).collect();
    assert!(row[..16].iter().all(|&pixel| pixel == Some(red)), "{row:?}");
    assert_eq!(row[16], Some(Rgb::BLACK));
    assert_eq!(&picture(&frame)[0][..16], "....########....");
    // A TrueType font's background fills both cells.
    let mut grid = outline_grid();
    let (width, height) = grid.cell();
    let mut frame = Frame::new(width * 3, height);
    grid.draw_text(&mut frame, 0, 0, Text { c: ' ', ..text }, &[]);
    assert_eq!(frame.pixel(2 * width - 1, 0), Some(red));
    assert_eq!(frame.pixel(2 * width, 0), Some(Rgb::BLACK));
}

#[test]
fn the_glyphs_of_neighbours_reach_into_the_cell() {
    let mut grid = outline_grid();
    let (width, height) = grid.cell();
    let mut frame = Frame::new(width, height);
    let blank = Text {
        c: ' ',
        style: style(false),
        span: 1,
    };
    let lit = |frame: &Frame| {
        (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .filter(|&(x, y)| frame.pixel(x, y) != Some(Rgb::BLACK))
            .count()
    };
    // An M whose cell starts half a cell to the left reaches into this one.
    let half = -i64::from(width / 2);
    let m = Neighbour {
        c: 'M',
        fg: WHITE,
        face: FontFace::Regular,
        offset: half,
    };
    grid.draw_text(&mut frame, 0, 0, blank, &[m]);
    assert!(lit(&frame) > 0);
    // From the right too; nothing from a neighbour a cell away, or from a
    // shape, which stays in its cell.
    for (c, offset) in [
        ('M', i64::from(width / 2)),
        ('M', i64::from(width)),
        ('█', half),
    ] {
        let mut frame = Frame::new(width, height);
        let neighbour = Neighbour { c, offset, ..m };
        grid.draw_text(&mut frame, 0, 0, blank, &[neighbour]);
        assert_eq!(
            lit(&frame) > 0,
            offset == i64::from(width / 2),
            "{c} {offset}"
        );
    }
}

#[test]
fn a_console_font_without_a_shape_has_it_drawn() {
    let mut grid = Grid::new(font(), 1);
    let mut frame = Frame::new(24, 2);
    // The font has no full block, no lower half and no Braille: drawn, not
    // a question mark. A character it lacks that is not a shape stays `?`.
    grid.draw(&mut frame, 0, 0, '█', style(false));
    grid.draw(&mut frame, 1, 0, '▄', style(false));
    grid.draw(&mut frame, 2, 0, 'é', style(false));
    assert_eq!(
        picture(&frame),
        ["########........#.......", "#################......."]
    );
}
