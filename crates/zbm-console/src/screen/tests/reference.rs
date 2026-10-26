//! Reference images (C109): a test card drawn by the console in memory with
//! three fonts, compared with the PNG files in `reference/`, as the menu's
//! wireframes are compared with text. Change the drawing, then look at the
//! new images and accept them with `cargo insta review` or
//! `cargo xtask bless`.
//!
//! The card holds the 16 colours, 256 colours and true colour, the
//! attributes and faces, the five underlines and strikeout, wide characters
//! and combining marks, the shapes the console draws itself (lines,
//! blocks, Braille, Powerline, sextants), and a cursor, a different shape
//! for each font.

use std::fmt::Write as _;

use zbm_display::faces::{Faces, FontFace};
use zbm_display::outline::Outline;
use zbm_testkit::png::png;

use super::*;

/// The card's size in cells.
const COLUMNS: u32 = 34;
const ROWS: u32 = 10;

/// The faces of the image's TrueType font, `DejaVuSansMono`.
const DEJAVU: [(FontFace, &[u8]); 4] = [
    (
        FontFace::Regular,
        include_bytes!("../../../../../legacy/zfsbootmenu/fonts/dejavu/DejaVuSansMono.ttf"),
    ),
    (
        FontFace::Bold,
        include_bytes!("../../../../../legacy/zfsbootmenu/fonts/dejavu/DejaVuSansMono-Bold.ttf"),
    ),
    (
        FontFace::Italic,
        include_bytes!("../../../../../legacy/zfsbootmenu/fonts/dejavu/DejaVuSansMono-Oblique.ttf"),
    ),
    (
        FontFace::BoldItalic,
        include_bytes!(
            "../../../../../legacy/zfsbootmenu/fonts/dejavu/DejaVuSansMono-BoldOblique.ttf"
        ),
    ),
];

/// The image's Terminus 8×14 in cells of `scale` times its size.
fn terminus(scale: u32) -> Grid {
    Grid::new(Font::parse(TER_V14B).unwrap(), scale)
}

/// The image's TrueType font and its faces in cells `height` pixels high.
fn dejavu(height: u32) -> Grid {
    let outline = |bytes: &[u8]| Outline::new(bytes.to_vec(), height).unwrap();
    let [(_, regular), others @ ..] = DEJAVU;
    let faces = others
        .into_iter()
        .fold(Faces::new(outline(regular)), |faces, (face, bytes)| {
            faces.with_face(face, outline(bytes))
        });
    Grid::faces(faces)
}

/// The rows of the card, each ending with a new line but the last.
fn card() -> String {
    let mut card = String::new();
    // The 16 colours as backgrounds, then as text.
    for colour in (40..48).chain(100..108) {
        write!(card, "\x1b[{colour}m  ").unwrap();
    }
    card += "\x1b[0m\r\n";
    // 24 greys and 8 colours of the 256, then true colour.
    for colour in (232..256).chain([196, 202, 208, 214, 220, 226, 190, 154]) {
        write!(card, "\x1b[48;5;{colour}m ").unwrap();
    }
    card += "\x1b[0m\r\n";
    for step in 0..32 {
        write!(card, "\x1b[48;2;{};{};128m ", step * 8, 255 - step * 8).unwrap();
    }
    card += "\x1b[0m\r\n";
    card += "\x1b[1mBold\x1b[0m \x1b[3mItalic\x1b[0m \x1b[1;3mBoth\x1b[0m ";
    card += "\x1b[2mDim\x1b[0m \x1b[7mInverse\x1b[0m\r\n";
    card += "\x1b[4mone\x1b[0m \x1b[21mtwo\x1b[0m \x1b[4:3;58;5;196mcurl\x1b[0m ";
    card += "\x1b[4:4mdot\x1b[0m \x1b[4:5mdash\x1b[0m \x1b[9mstrike\x1b[0m\r\n";
    card += "\x1b[31mred \x1b[1;31mbright\x1b[0m \x1b[38;5;208morange\x1b[0m ";
    card += "\x1b[38;2;80;160;255mtrue\x1b[0m\r\n";
    card += "日本語 e\u{301} ñ ┌─┬─┐ ╭─╮ ╱╲╳ ═║╬\r\n";
    card += "⣿⡇⠿⢸ \u{e0b0}\u{e0b1}\u{e0b2}\u{e0b3} 🬀🬁🬂🬃 ▀▄█▌▐░▒▓\r\n";
    card += "\x1b[44;97m status \x1b[0m\u{e0b0} ok\r\n";
    card += "$ ";
    card
}

/// The card drawn with `grid`, a cursor of DECSCUSR `shape`, as a PNG
/// file.
fn drawn(grid: Grid, shape: u8) -> Vec<u8> {
    let (width, height) = grid.cell();
    let mut console = Console::new(grid, (COLUMNS * width, ROWS * height));
    console.feed(format!("\x1b[{shape} q{}", card()).as_bytes());
    console.draw();
    let frame = console.frame_mut();
    let (width, height) = frame.size();
    let rows: Vec<Vec<(u8, u8, u8)>> = (0..height)
        .map(|y| {
            frame
                .row(y)
                .unwrap()
                .iter()
                .map(|&pixel| {
                    let Rgb(r, g, b) = Rgb::from_xrgb(pixel);
                    (r, g, b)
                })
                .collect()
        })
        .collect();
    png(width, &rows)
}

#[test]
fn the_test_card_looks_as_its_reference_images() {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("reference");
    settings.set_prepend_module_to_snapshot(false);
    settings.set_omit_expression(true);
    settings.bind(|| {
        // A steady block, underline and bar.
        insta::assert_binary_snapshot!("terminus-8x14.png", drawn(terminus(1), 2));
        insta::assert_binary_snapshot!("dejavu-20.png", drawn(dejavu(20), 4));
        insta::assert_binary_snapshot!("dejavu-32.png", drawn(dejavu(32), 6));
    });
}
