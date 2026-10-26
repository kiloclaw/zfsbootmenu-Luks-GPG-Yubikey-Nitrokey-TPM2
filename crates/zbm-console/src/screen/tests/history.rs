//! The scrollback: kept, shown with Shift+PgUp, and wiped for secret
//! prompts.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::*;

fn shift(key: Key) -> KeyPress {
    KeyPress {
        key,
        shift: true,
        ..KeyPress::BLANK
    }
}

/// A console of 5 rows with a scrollback of 20 lines, after 12 numbered
/// lines.
fn scrolled() -> Console {
    let mut console = console().with_scrollback(20);
    for line in 1..=12 {
        console.feed(format!("line {line}\r\n").as_bytes());
    }
    console
}

#[test]
fn shift_page_up_shows_the_lines_that_scrolled_off() {
    let mut console = scrolled();
    assert_eq!(console.history_lines(), 8);
    assert!(!console.is_scrolled_back());
    // Without Shift, Page Up is the programs'.
    let page_up = KeyPress {
        key: Key::PageUp,
        ..KeyPress::BLANK
    };
    assert_eq!(console.take_key(page_up), Some(page_up));
    assert_eq!(console.take_key(shift(Key::PageUp)), None);
    assert!(console.is_scrolled_back());
    console.draw();
    assert_eq!(
        console.text(),
        "line 4         [5/8]\nline 5\nline 6\nline 7\nline 8\n"
    );
    assert_eq!(console.take_key(shift(Key::PageDown)), None);
    assert!(!console.is_scrolled_back());
    let _ = console.take_key(shift(Key::PageUp));
    console.scroll_to_bottom();
    assert_eq!(console.text().lines().next(), Some("line 9"));
    assert_eq!(console.take_key(shift(Key::Home)), Some(shift(Key::Home)));
}

#[test]
fn a_secret_prompt_wipes_the_scrollback_and_keeps_none() {
    let mut console = scrolled();
    let _ = console.take_key(shift(Key::PageUp));
    console.set_secret(true);
    assert!(console.is_secret());
    assert!(!console.is_scrolled_back());
    assert_eq!(console.history_lines(), 0);
    console.feed(b"more\r\nand more\r\n");
    assert_eq!(console.history_lines(), 0);
    assert_eq!(
        console.take_key(shift(Key::PageUp)),
        Some(shift(Key::PageUp))
    );
    // Once it is closed, history is kept again, from then on.
    console.set_secret(false);
    console.set_secret(false);
    for line in 1..=6 {
        console.feed(format!("after {line}\r\n").as_bytes());
    }
    assert_eq!(console.history_lines(), 6);
    // Lines on the screen at the prompt were not wiped; older ones were.
    let _ = console.take_key(shift(Key::PageUp));
    let _ = console.take_key(shift(Key::PageUp));
    let text = console.text();
    assert!(text.starts_with("line 11        [6/6]\n"), "{text:?}");
    assert!(!text.contains("line 10"));
}

#[test]
fn the_alternate_screen_and_a_wipe_leave_no_history_to_show() {
    let mut console = scrolled();
    console.feed(b"\x1b[?1049h");
    assert_eq!(
        console.take_key(shift(Key::PageUp)),
        Some(shift(Key::PageUp))
    );
    console.feed(b"\x1b[?1049l");
    // Clearing the history (`clear`) and wiping the console drop it.
    console.feed(b"\x1b[3J");
    assert_eq!(console.history_lines(), 0);
    let mut console = scrolled();
    console.wipe();
    assert_eq!(console.history_lines(), 0);
    assert_eq!(console.text(), "\n\n\n\n\n");
}
