//! The cell surface: whole frames drawn, only what changed, over the
//! terminal and under the bars.

#![allow(
    clippy::disallowed_methods,
    reason = "the status line is said at a real instant"
)]

use pretty_assertions::assert_eq;
use zbm_display::decor::Decoration;
use zbm_display::frame::{Area, Rgb};
use zbm_display::grid::Grid;
use zbm_display::psf::Font;

use super::*;
use crate::screen::Status;

/// The image's Terminus 8×14.
const TER_V14B: &[u8] = include_bytes!("../../../../../legacy/zfsbootmenu/fonts/ter-v14b.psf");

const RED: Rgb = Rgb(200, 0, 0);
const BLUE: Rgb = Rgb(0, 0, 200);

/// A terminal of 20×5 cells of 8×14 pixels, its first frame drawn.
fn console() -> Console {
    let mut console = Console::new(
        Grid::new(Font::parse(TER_V14B).unwrap(), 1),
        (20 * 8, 5 * 14),
    );
    console.draw();
    console
}

fn style(fg: Rgb, bg: Rgb) -> CellStyle {
    CellStyle {
        fg,
        bg,
        decoration: Decoration::default(),
        face: FontFace::Regular,
    }
}

fn run(row: u16, column: u16, cells: &[&str], style: CellStyle) -> Run {
    Run {
        row,
        column,
        style,
        cells: cells.iter().map(|cell| (*cell).to_owned()).collect(),
    }
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

#[test]
fn a_frame_shows_once_it_is_whole_and_the_terminal_after_it() {
    let mut console = console();
    console.feed(b"shell");
    console.draw();
    console.show_cells();
    assert!(console.is_showing_cells());
    // Blank, drawn whole, until the client's first frame.
    assert_eq!(console.draw(), 100);
    assert_eq!(console.text(), "\n\n\n\n\n");

    console.put_cells(&[run(1, 2, &["m", "e", "n", "u"], style(RED, BLUE))]);
    // Half a frame does not show.
    assert_eq!(console.draw(), 0);
    console.end_cells_frame(None);
    // The four cells and the one on either side.
    assert_eq!(console.draw(), 6);
    assert_eq!(console.text(), "\n  menu\n\n\n\n");
    assert_eq!(colours(&mut console, 3, 1), [BLUE, RED]);

    // The programs write meanwhile, and show once the client lends the
    // screen.
    console.feed(b" on");
    assert_eq!(console.draw(), 0);
    console.show_terminal();
    assert!(!console.is_showing_cells());
    assert!(console.draw() > 0);
    assert_eq!(console.text(), "shell on\n\n\n\n\n");
    // Lending it twice changes nothing.
    console.show_terminal();
    assert!(!console.is_showing_cells());
}

#[test]
fn only_what_changed_is_drawn_again_with_the_cursor() {
    let mut console = console();
    console.show_cells();
    console.draw();
    let plain = style(RED, BLUE);
    console.put_cells(&[run(0, 0, &["a", "b", "c"], plain)]);
    console.end_cells_frame(Some((1, 0)));
    console.draw();
    // Under the cursor, the colours are reversed.
    assert_eq!(colours(&mut console, 1, 0), [BLUE, RED]);
    console.frame_mut().take_damage();

    // The same cells again change nothing; the cursor moves.
    console.put_cells(&[run(0, 0, &["a", "b", "c"], plain)]);
    console.end_cells_frame(Some((2, 0)));
    assert_eq!(console.draw(), 2);
    let damage = console.frame_mut().take_damage();
    assert_eq!(
        damage,
        [Area {
            x: 8,
            y: 0,
            width: 16,
            height: 14
        }]
    );

    // Beyond the surface, nothing is kept.
    console.put_cells(&[
        run(0, 18, &["x", "y", "z"], plain),
        run(9, 0, &["q"], plain),
    ]);
    console.end_cells_frame(None);
    assert_eq!(console.draw(), 4);
    assert_eq!(console.text(), "abc               xy\n\n\n\n\n");
}

#[test]
fn wide_characters_marks_and_control_characters_are_drawn_as_shown() {
    let mut console = console();
    console.show_cells();
    console.draw();
    let plain = style(RED, BLUE);
    let cells = ["日", "", "e\u{301}", "\x0c", "\n"];
    console.put_cells(&[run(2, 0, &cells, plain)]);
    console.end_cells_frame(Some((1, 2)));
    assert!(console.draw() > 0);
    assert_eq!(console.text(), "\n\n日e\u{301}\n\n\n");
    // The second half of the wide character is drawn with the first.
    assert_eq!(colours(&mut console, 1, 2).len(), 2);
}

#[test]
fn the_status_line_and_a_notice_show_over_the_cells() {
    let mut console = console().with_status_line();
    console.show_cells();
    assert_eq!(console.size(), (20, 4));
    let status = Status {
        layout: "de".to_owned(),
        ..Status::default()
    };
    console.set_status(&status, std::time::Instant::now());
    console.draw();
    console.put_cells(&[run(3, 0, &["z"; 20], style(RED, BLUE))]);
    console.end_cells_frame(None);
    console.draw();
    let text = console.text();
    assert!(
        text.starts_with(&format!("\n\n\n{}\n", "z".repeat(20))),
        "{text}"
    );
    assert!(text.ends_with(" de\n"), "{text}");

    console.set_notice("Saved");
    console.draw();
    assert!(console.text().contains("\nSaved\n"), "{}", console.text());

    // A new size blanks the cells, for the client to draw again.
    console.set_grid(Grid::new(Font::parse(TER_V14B).unwrap(), 1));
    assert!(console.is_showing_cells());
    console.draw();
    assert!(!console.text().contains('z'));
}

#[test]
fn the_keys_of_search_and_scrolling_are_the_clients() {
    use zbm_display::keys::{Key, KeyPress};
    let mut console = console().with_scrollback(100);
    console.show_cells();
    let page_up = KeyPress::new(Key::PageUp).with_shift();
    assert_eq!(console.take_key(page_up), Some(page_up));
    let search = KeyPress::new(Key::Char('F')).with_ctrl().with_shift();
    assert_eq!(console.take_key(search), Some(search));
    // The font's size is still the console's.
    let bigger = KeyPress::new(Key::Char('+')).with_ctrl().with_alt();
    assert_eq!(console.take_key(bigger), None);
}

#[test]
fn the_cells_are_wiped_with_the_screen() {
    let mut console = console();
    console.show_cells();
    console.put_cells(&[run(0, 0, &["s"], style(RED, BLUE))]);
    console.wipe();
    assert!(!console.is_showing_cells());
    // Without cells, nothing is put.
    console.put_cells(&[run(0, 0, &["s"], style(RED, BLUE))]);
    console.end_cells_frame(None);
    console.draw();
    assert!(!console.text().contains('s'));
}
