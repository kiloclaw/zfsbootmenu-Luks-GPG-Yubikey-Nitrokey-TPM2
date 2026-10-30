use pretty_assertions::assert_eq;

use super::*;
use crate::decor::Decoration;
use crate::faces::FontFace;
use crate::outline::Outline;
use crate::psf::Font;

/// The image's console font at 16 pixels.
fn psf() -> Grid {
    let bytes = include_bytes!("../../../../legacy/zfsbootmenu/fonts/ter-v14b.psf");
    Grid::new(Font::parse(bytes).unwrap(), 1)
}

/// The image's TrueType font in cells 20 pixels high.
fn ttf() -> Grid {
    let font = crate::outline::tests::DEJAVU.to_vec();
    Grid::outline(Outline::new(font, 20).unwrap())
}

fn style(fg: Rgb, face: FontFace, underline: bool) -> CellStyle {
    CellStyle {
        fg,
        bg: Rgb(0, 0, 40),
        decoration: Decoration::underlined(underline),
        face,
    }
}

/// What a frame shows: every pixel.
fn pixels(frame: &Frame) -> Vec<u32> {
    let (_, height) = frame.size();
    (0..height)
        .flat_map(|y| frame.row(y).unwrap().to_vec())
        .collect()
}

/// Draws `cells` (column, character, style, span) with `grid` directly
/// and through a renderer, twice, so that the second pass comes from the
/// cache; the three frames.
fn drawn(make: fn() -> Grid, cells: &[(u16, char, CellStyle, u16)]) -> [Vec<u32>; 3] {
    let mut grid = make();
    let (width, height) = grid.cell();
    let size = (width * 8, height);
    let mut direct = Frame::new(size.0, size.1);
    for &(column, c, style, span) in cells {
        grid.draw_text(&mut direct, column, 0, Text { c, style, span }, &[]);
    }
    let mut renderer = CellRenderer::new(make());
    let mut passes = Vec::new();
    for _ in 0..2 {
        let mut frame = Frame::new(size.0, size.1);
        for &(column, c, style, span) in cells {
            renderer.draw_text(&mut frame, column, 0, Text { c, style, span }, &[]);
        }
        passes.push(pixels(&frame));
    }
    let [first, second] = <[Vec<u32>; 2]>::try_from(passes).unwrap();
    [pixels(&direct), first, second]
}

#[test]
fn cached_cells_are_the_cells_the_grid_draws() {
    let white = Rgb(255, 255, 255);
    let cells = [
        (0, 'A', style(white, FontFace::Regular, false), 1),
        (1, 'g', style(Rgb(200, 30, 30), FontFace::Bold, true), 1),
        (2, '─', style(white, FontFace::Regular, false), 1),
        (3, '漢', style(white, FontFace::Regular, false), 2),
        (5, '▶', style(Rgb(0, 200, 200), FontFace::Italic, false), 1),
        (6, 'A', style(white, FontFace::Regular, false), 1),
    ];
    for make in [psf, ttf] {
        let [direct, first, cached] = drawn(make, &cells);
        assert_eq!(first, direct);
        assert_eq!(cached, direct);
    }
}

#[test]
fn each_character_and_style_is_drawn_once() {
    let mut renderer = CellRenderer::new(psf());
    let (width, height) = renderer.cell();
    let mut frame = Frame::new(width * 4, height);
    let plain = style(Rgb(255, 255, 255), FontFace::Regular, false);
    for column in 0..4 {
        renderer.draw(&mut frame, column, 0, 'x', plain);
    }
    renderer.draw(
        &mut frame,
        0,
        0,
        'x',
        style(Rgb(1, 2, 3), FontFace::Regular, false),
    );
    assert_eq!(renderer.cached(), 2);
    assert_eq!(renderer.name(), "psf");
    assert_eq!(renderer.dimensions((width * 4, height)), (4, 1));
    assert_eq!(renderer.area(1, 0).x, width);

    renderer.warm(warm_chars(), plain);
    assert_eq!(renderer.cached(), 1 + 95 + 11);
    // Full, the cache starts again.
    for c in ('\u{4e00}'..).take(CAPACITY) {
        renderer.draw(&mut frame, 0, 0, c, plain);
    }
    assert!(renderer.cached() < CAPACITY);
}

#[test]
fn a_cell_a_neighbour_reaches_into_is_drawn_with_it() {
    let italic = style(Rgb(255, 255, 255), FontFace::Italic, false);
    let neighbour = |offset| Neighbour {
        c: 'M',
        fg: italic.fg,
        face: FontFace::Regular,
        offset,
    };
    let mut grid = ttf();
    let (width, height) = grid.cell();
    // An M whose cell starts half a cell to the left.
    let offset = -i64::from(width / 2);
    let mut direct = Frame::new(width * 2, height);
    let text = Text {
        c: ' ',
        style: italic,
        span: 1,
    };
    grid.draw_text(&mut direct, 1, 0, text, &[neighbour(offset)]);
    let mut renderer = CellRenderer::new(ttf());
    let mut frame = Frame::new(width * 2, height);
    renderer.draw_text(&mut frame, 1, 0, text, &[neighbour(offset)]);

    assert!(grid.is_reaching(&neighbour(offset), 1));
    assert!(!grid.is_reaching(&neighbour(-i64::from(width)), 1));
    assert!(!psf().is_reaching(&neighbour(offset), 1));
    assert_eq!(pixels(&frame), pixels(&direct));
    // Not cached: it depends on the neighbour.
    assert_eq!(renderer.cached(), 0);
    renderer.draw_mark(&mut frame, 1, 0, '\u{301}', italic.fg);
    assert_eq!(renderer.grid().name(), "ttf");
}
