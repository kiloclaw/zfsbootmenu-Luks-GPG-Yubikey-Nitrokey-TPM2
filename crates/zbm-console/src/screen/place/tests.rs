use pretty_assertions::assert_eq;
use zbm_display::frame::Rgb;
use zbm_display::grid::Grid;
use zbm_display::psf::Font;

use super::*;

/// The image's Terminus 8×14.
const TER_V14B: &[u8] = include_bytes!("../../../../../legacy/zfsbootmenu/fonts/ter-v14b.psf");

/// A terminal of 20×6 cells of 8×14 pixels.
fn console() -> Console {
    Console::new(
        Grid::new(Font::parse(TER_V14B).unwrap(), 1),
        (20 * 8, 6 * 14),
    )
}

/// Whether any pixel of the cell-sized block at `x`, `y` is lit.
fn is_lit(console: &mut Console, (x, y): (u32, u32)) -> bool {
    let frame = console.frame_mut();
    (y..y + 14)
        .any(|y| (x..x + 8).any(|x| frame.pixel(x, y).is_some_and(|pixel| pixel != Rgb(0, 0, 0))))
}

#[test]
fn a_held_screen_is_that_size_centred_and_fills_the_display_again_after() {
    let mut console = console();
    assert_eq!(console.size(), (20, 6));

    assert!(console.hold_size(Some((10, 2))).is_some());
    assert_eq!(console.size(), (10, 2));
    assert_eq!(console.held_size(), Some((10, 2)));
    assert_eq!(console.window_size().num_cols, 10);
    // Centred: 5 cells of margin on each side, 2 rows above and below.
    assert_eq!(console.grid().origin(), (5 * 8, 2 * 14));
    console.feed(b"\x1b[7mX");
    console.draw();
    assert!(is_lit(&mut console, (5 * 8, 2 * 14)));
    assert!(!is_lit(&mut console, (0, 0)));
    // Held again to the same size: nothing to tell the programs.
    assert!(console.hold_size(Some((10, 2))).is_none());

    assert!(console.hold_size(None).is_some());
    assert_eq!(console.size(), (20, 6));
    assert_eq!(console.grid().origin(), (0, 0));
}

#[test]
fn a_held_size_larger_than_the_display_is_the_display_s() {
    let mut console = console();
    assert!(console.hold_size(Some((132, 50))).is_some());
    assert_eq!(console.size(), (20, 6));
    assert_eq!(console.grid().origin(), (0, 0));
}

#[test]
fn the_margins_are_around_the_cells() {
    let mut console = console();
    console.hold_size(Some((10, 2)));
    let [left, right, top, bottom] = console.margins();
    assert_eq!((left.width, right.x, right.width), (40, 120, 40));
    assert_eq!((top.height, bottom.y, bottom.height), (28, 56, 28));
}
