//! The status line, the title, the bell and screenshots.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::*;
use crate::screen::bell::FLASH;
use crate::screen::{Clock, Locks, Ring, Status};

fn status() -> Status {
    Status {
        layout: "de".to_owned(),
        locks: Locks {
            caps: true,
            num: false,
            scroll: true,
        },
        clock: Some(Clock::of(3600 * 25 + 60 * 7 + 42)),
        battery: Some(87),
    }
}

#[test]
fn the_line_has_the_title_on_the_left_and_the_rest_on_the_right() {
    let line = |title: Option<&str>, columns| {
        status()
            .line(title, columns)
            .into_iter()
            .collect::<String>()
    };
    assert_eq!(
        line(Some("vim"), 48),
        " vim   de  Caps  Scroll  01:07 UTC  Battery 87% "
    );
    // A long title is cut, a space kept before the rest; a line too
    // narrow keeps the end of the rest.
    assert_eq!(
        line(Some("a very long title"), 47),
        " a ve de  Caps  Scroll  01:07 UTC  Battery 87% "
    );
    assert_eq!(line(None, 10), "ttery 87% ");
    assert_eq!(
        Clock::of(86_399),
        Clock {
            hour: 23,
            minute: 59,
            second: 59
        }
    );
    let bare = Status::default()
        .line(None, 8)
        .into_iter()
        .collect::<String>();
    assert_eq!(bare, "        ");
}

#[test]
fn the_status_line_takes_a_row_from_the_programs_and_says_the_title() {
    let mut console = console().with_status_line().with_status_line();
    assert!(console.has_status_line());
    assert_eq!(console.size(), (20, 4));
    assert_eq!(console.window_size().num_lines, 4);
    console.feed(b"\x1b]2;shell\x07one\r\ntwo");
    assert_eq!(console.title(), Some("shell"));
    let now = Instant::now();
    let narrow = Status {
        clock: None,
        battery: None,
        ..status()
    };
    console.set_status(&narrow, now);
    console.draw();
    assert_eq!(console.text(), "one\ntwo\n\n\n s de  Caps  Scroll\n");
    // The status row is drawn in the default colours reversed.
    assert!(colours(&mut console, 1, 4).contains(&GREY));
    // With the time, the console wakes for the next minute.
    console.set_status(&status(), now);
    assert_eq!(console.deadline(), Some(now + Duration::from_secs(18)));
    console.feed(b"\x1b]2;\x07\x1b]104\x07");
    // Without a status line, nothing is said.
    let mut plain = console_without_status();
    plain.set_status(&status(), now);
    assert_eq!(plain.text(), "\n\n\n\n\n");
    // A new grid keeps the status line.
    console.set_grid(Grid::new(Font::parse(TER_V14B).unwrap(), 1));
    assert_eq!(console.size(), (20, 4));
}

fn console_without_status() -> Console {
    console()
}

#[test]
fn the_bell_flashes_the_border_or_asks_the_speaker() {
    let start = Instant::now();
    let mut console = console();
    console.draw();
    console.feed(b"\x07");
    assert_eq!(console.take_sounds(), 0);
    console.tick(start);
    assert_eq!(console.bell.at(), Some(start + FLASH));
    assert!(console.draw() > 0);
    // The border, in the default text colour, here on the right edge.
    let edge = console.frame_mut().pixel(20 * 8 - 1, 35).unwrap();
    assert_eq!(edge, GREY);
    // A bell during the flash does not start another.
    console.feed(b"\x07");
    console.tick(start + FLASH / 2);
    console.tick(start + FLASH);
    assert_eq!(console.bell.at(), None);
    console.draw();
    assert_eq!(
        console.frame_mut().pixel(20 * 8 - 1, 35).unwrap(),
        Rgb::BLACK
    );

    let mut audible = super::console().with_bell(Ring::Audible);
    audible.feed(b"\x07\x07");
    audible.tick(start);
    assert_eq!(audible.take_sounds(), 2);
    assert_eq!(audible.take_sounds(), 0);
    assert_eq!(audible.bell.at(), None);
    let mut off = super::console().with_bell(Ring::Off);
    off.feed(b"\x07");
    off.tick(start);
    assert_eq!(off.take_sounds(), 0);
    assert_eq!(off.bell.at(), None);
    assert_eq!(Ring::parse("both"), Some(Ring::Both));
    assert_eq!(Ring::parse("loud"), None);
}

#[test]
fn print_screen_takes_the_text_and_the_image_but_not_at_a_secret_prompt() {
    let print = KeyPress {
        key: Key::Print,
        ..KeyPress::BLANK
    };
    let mut console = console();
    console.feed(b"zpool status");
    console.draw();
    assert_eq!(console.take_screenshot(), None);
    assert_eq!(console.take_key(print), None);
    let (text, image) = console.take_screenshot().unwrap();
    assert!(text.starts_with("zpool status\n"));
    assert_eq!(image, zbm_display::png::of_frame(console.frame_mut()));
    assert_eq!(console.take_screenshot(), None);
    // With Ctrl, the programs'.
    let ctrl = KeyPress {
        ctrl: true,
        ..print
    };
    assert_eq!(console.take_key(ctrl), Some(ctrl));
    console.set_secret(true);
    assert_eq!(console.take_key(print), None);
    assert_eq!(console.take_screenshot(), None);
    assert!(console.text().ends_with("\nNo screenshot at a s\n"));
}
