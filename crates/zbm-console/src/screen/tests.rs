//! The terminal: text, colours, the cursor, the answers to the programs,
//! and nothing kept beyond the screen.

#![allow(
    clippy::disallowed_methods,
    reason = "the cursor's blinking is timed from a real instant"
)]

use std::time::{Duration, Instant};

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

    // Without a cursor, nothing blinks: the deadline is the update's.
    console.feed(b"\x1b[?25l\x1b[?2026hpartial");
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

#[test]
fn a_combining_mark_is_drawn_over_its_character() {
    let dejavu = include_bytes!("../../../../legacy/zfsbootmenu/fonts/dejavu/DejaVuSansMono.ttf");
    let outline = zbm_display::outline::Outline::new(dejavu.to_vec(), 20).unwrap();
    let grid = Grid::outline(outline);
    let (width, height) = grid.cell();
    let mut plain = Console::new(grid, (width * 4, height * 2));
    let outline = zbm_display::outline::Outline::new(dejavu.to_vec(), 20).unwrap();
    let mut accented = Console::new(Grid::outline(outline), (width * 4, height * 2));

    plain.feed(b"e");
    accented.feed("e\u{301}".as_bytes());
    plain.draw();
    accented.draw();

    let pixels = |console: &mut Console| -> Vec<u32> {
        let frame = console.frame_mut();
        (0..height)
            .flat_map(|y| frame.row(y).unwrap()[..usize::try_from(width).unwrap()].to_vec())
            .collect()
    };
    assert_ne!(pixels(&mut plain), pixels(&mut accented));
    // The mark belongs to the e, in the screen's text too.
    assert_eq!(accented.text().lines().next(), Some("e\u{301}"));
}

#[test]
fn wiping_leaves_nothing_of_the_screen_in_the_cells_or_the_pixels() {
    let mut console = console();
    console.feed(b"\x1b[31mzpool secret\r\nline two\x1b[6n");
    console.draw();
    assert!(console.text().contains("secret"));

    console.wipe();

    assert!(console.text().trim().is_empty());
    assert!(console.take_replies().is_empty());
    for column in 0..20 {
        assert_eq!(colours(&mut console, column, 0), [Rgb::BLACK]);
    }
}

/// The pixels of the cell at `column`, `row` as text: `#` for `colour`,
/// `.` for anything else.
fn cell_picture(console: &mut Console, column: u32, row: u32, colour: Rgb) -> Vec<String> {
    let frame = console.frame_mut();
    (row * 14..(row + 1) * 14)
        .map(|y| {
            (column * 8..(column + 1) * 8)
                .map(|x| {
                    if frame.pixel(x, y) == Some(colour) {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect()
}

#[test]
fn the_cursor_takes_the_shape_programs_ask_for() {
    let mut console = console();
    // DECSCUSR 4: a steady underline.
    console.feed(b"\x1b[4 q");
    console.draw();
    let underline = cell_picture(&mut console, 0, 0, GREY);
    assert_eq!(underline[13], "########");
    assert!(underline[..13].iter().all(|row| row == "........"));

    // 6: a steady bar, on the left.
    console.feed(b"\x1b[6 q");
    console.draw();
    let bar = cell_picture(&mut console, 0, 0, GREY);
    assert!(bar.iter().all(|row| row == "#......."), "{bar:?}");

    // 2: a steady block again.
    console.feed(b"\x1b[2 q");
    console.draw();
    assert_eq!(colours(&mut console, 0, 0), [GREY]);
}

#[test]
fn the_cursor_blinks_while_shown_and_shows_after_output() {
    let mut console = console();
    let start = Instant::now();
    console.tick(start);
    let first = console.deadline().unwrap();
    assert_eq!(first, start + BLINK);
    console.draw();
    assert_eq!(colours(&mut console, 0, 0), [GREY]);

    console.tick(first);
    console.draw();
    assert_eq!(colours(&mut console, 0, 0), [Rgb::BLACK]);
    console.tick(first + BLINK);
    console.draw();
    assert_eq!(colours(&mut console, 0, 0), [GREY]);

    // Output shows it again, and starts the blinking over.
    console.tick(first + 2 * BLINK);
    console.feed(b"x");
    console.tick(first + 2 * BLINK);
    console.draw();
    assert_eq!(colours(&mut console, 1, 0), [GREY]);
    assert_eq!(console.deadline(), Some(first + 3 * BLINK));

    // A steady cursor, or a hidden one, stops the timer.
    console.feed(b"\x1b[2 q");
    console.tick(first + 3 * BLINK);
    assert_eq!(console.deadline(), None);
    console.feed(b"\x1b[1 q\x1b[?25l");
    console.tick(first + 3 * BLINK);
    assert_eq!(console.deadline(), None);
}

#[test]
fn a_hollow_cursor_is_a_box() {
    let mut frame = Frame::new(8, 4);
    let area = Area {
        x: 0,
        y: 0,
        width: 8,
        height: 4,
    };
    draw_cursor(&mut frame, area, CursorShape::HollowBlock, GREY);
    draw_cursor(&mut frame, area, CursorShape::Hidden, Rgb::BLACK);

    let rows: Vec<String> = (0..4)
        .map(|y| {
            (0..8)
                .map(|x| {
                    if frame.pixel(x, y) == Some(GREY) {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect();
    assert_eq!(rows, ["########", "#......#", "#......#", "########"]);
}

/// Every pixel of `console`'s frame.
fn pixels(console: &mut Console) -> Vec<Rgb> {
    let frame = console.frame_mut();
    let (width, height) = frame.size();
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .map(|(x, y)| frame.pixel(x, y).unwrap())
        .collect()
}

/// How many pixels of `one` and `other` differ.
fn differing(one: &mut Console, other: &mut Console) -> usize {
    let theirs = pixels(other);
    pixels(one)
        .iter()
        .zip(&theirs)
        .filter(|(mine, theirs)| mine != theirs)
        .count()
}

#[test]
fn a_scrolled_screen_is_moved_and_looks_as_if_drawn_anew() {
    let mut scrolled = console();
    let mut fresh = console();
    let mut text = Vec::new();
    for line in 0..8 {
        text.extend_from_slice(format!("\x1b[3{}mline {line}\r\n", line % 8).as_bytes());
    }
    for line in text.split_inclusive(|&byte| byte == b'\n') {
        scrolled.feed(line);
        scrolled.draw();
    }
    // One more line: the screen scrolls by one, and only the rows that
    // came, the last line and the cursor's, are drawn.
    scrolled.feed(b"\x1b[0mlast\r\n");
    let drawn = scrolled.draw();
    assert!(drawn <= 1 + 2 * 20, "{drawn} cells drawn");

    fresh.feed(&text);
    fresh.feed(b"\x1b[0mlast\r\n");
    fresh.draw();
    assert_eq!(scrolled.text(), fresh.text());
    assert_eq!(differing(&mut scrolled, &mut fresh), 0);

    // Back down by one, as a reverse index at the top does.
    scrolled.feed(b"\x1b[H\x1bM");
    assert!(scrolled.draw() <= 1 + 2 * 20);
    let mut fresh = console();
    fresh.feed(&text);
    fresh.feed(b"\x1b[0mlast\r\n\x1b[H\x1bM");
    fresh.draw();
    assert_eq!(scrolled.text(), fresh.text());
    assert_eq!(differing(&mut scrolled, &mut fresh), 0);
}
