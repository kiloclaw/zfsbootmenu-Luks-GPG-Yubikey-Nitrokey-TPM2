//! The terminal: text, colours, the cursor, the answers to the programs,
//! and nothing kept beyond the screen.

use std::time::Duration;

use pretty_assertions::assert_eq;
use zbm_display::frame::Rgb;
use zbm_types::psf::Font;

use super::*;

/// The image's Terminus 8×14.
const TER_V14B: &[u8] = include_bytes!("../../../../legacy/zfsbootmenu/fonts/ter-v14b.psf");

/// A terminal of 20×5 cells of 8×14 pixels.
fn console() -> Console {
    Console::new(
        Grid::new(Font::parse(TER_V14B).unwrap(), 1),
        (20 * 8, 5 * 14),
    )
}

/// The colours of the pixels of the cell at `column`, `row`.
fn colours(console: &mut Console, column: u32, row: u32) -> Vec<Rgb> {
    let frame = console.frame_mut();
    let mut found = Vec::new();
    for y in row * 14..(row + 1) * 14 {
        for x in column * 8..(column + 1) * 8 {
            let pixel = frame.pixel(x, y).unwrap();
            if !found.contains(&pixel) {
                found.push(pixel);
            }
        }
    }
    found.sort_by_key(|rgb| (rgb.0, rgb.1, rgb.2));
    found
}

const GREY: Rgb = Rgb(0xaa, 0xaa, 0xaa);

#[test]
fn the_screen_holds_what_the_programs_wrote() {
    let mut console = console();
    console.feed("hello\r\nwörld 日本\r\n".as_bytes());

    assert_eq!(console.size(), (20, 5));
    assert_eq!(console.text(), "hello\nwörld 日本\n\n\n\n");
    let size = console.window_size();
    assert_eq!(
        (
            size.num_cols,
            size.num_lines,
            size.cell_width,
            size.cell_height
        ),
        (20, 5, 8, 14)
    );
}

#[test]
fn cells_are_drawn_in_their_colours_and_the_cursor_as_a_block() {
    let mut console = console();
    console.feed(b"\x1b[31mR\x1b[1mB\x1b[0m\x1b[38;5;196mX\x1b[48;2;1;2;3m \x1b[0m");
    assert!(console.draw() >= 5);

    assert_eq!(colours(&mut console, 0, 0), [Rgb::BLACK, Rgb(0xaa, 0, 0)]);
    assert_eq!(
        colours(&mut console, 1, 0),
        [Rgb::BLACK, Rgb(0xff, 0x55, 0x55)]
    );
    assert_eq!(colours(&mut console, 2, 0), [Rgb::BLACK, Rgb(0xff, 0, 0)]);
    assert_eq!(colours(&mut console, 3, 0), [Rgb(1, 2, 3)]);
    // The cursor, on the blank cell after them.
    assert_eq!(colours(&mut console, 4, 0), [GREY]);

    console.feed(b"\x1b[?25l");
    console.draw();
    assert_eq!(colours(&mut console, 4, 0), [Rgb::BLACK]);
    assert!(console.frame_mut().take_damage().is_some());
}

#[test]
fn the_programs_get_their_answers() {
    let mut console = console();
    console.feed(b"ab\x1b[6n");
    assert_eq!(console.take_replies(), b"\x1b[1;3R");
    assert_eq!(console.take_replies(), b"");

    console.feed(b"\x1b[c");
    assert!(console.take_replies().starts_with(b"\x1b[?"));
    console.feed(b"\x1b[14t");
    assert_eq!(console.take_replies(), b"\x1b[4;70;160t");
    console.feed(b"\x1b]4;1;?\x07");
    let colour = String::from_utf8(console.take_replies()).unwrap();
    assert!(colour.contains("rgb:aaaa/0000/0000"), "{colour}");
    // Clipboard requests are refused.
    console.feed(b"\x1b]52;c;?\x07");
    assert_eq!(console.take_replies(), b"");
}

#[test]
fn modes_and_synchronized_updates() {
    let mut console = console();
    assert!(!console.is_application_cursor());
    console.feed(b"\x1b[?1h");
    assert!(console.is_application_cursor());

    console.feed(b"\x1b[?2026hpartial");
    let deadline = console.deadline().unwrap();
    assert_eq!(console.text().lines().next(), Some(""));
    console.tick(deadline.checked_sub(Duration::from_millis(1)).unwrap());
    assert!(console.deadline().is_some());
    console.tick(deadline);
    assert_eq!(console.deadline(), None);
    assert_eq!(console.text().lines().next(), Some("partial"));
}

#[test]
fn nothing_is_kept_beyond_the_screen() {
    let mut console = console();
    for line in 0..50 {
        console.feed(format!("secret {line}\r\n").as_bytes());
    }

    assert_eq!(console.term.grid().history_size(), 0);
    assert!(!console.text().contains("secret 0\n"));
    assert!(console.text().contains("secret 49"));
}

#[test]
fn the_console_is_at_least_one_cell() {
    let console = Console::new(Grid::new(Font::parse(TER_V14B).unwrap(), 1), (3, 3));
    assert_eq!(console.size(), (1, 1));
    assert!(format!("{console:?}").starts_with("Console"));
}
