#![allow(
    clippy::disallowed_methods,
    reason = "the times of a sequence's wait are counted from a real instant"
)]

use std::time::Duration;

use pretty_assertions::assert_eq;
use zbm_display::decor::Decoration;

use super::*;
use crate::scheme::LINUX;

/// A cell of `text` in `fg` on `bg`.
fn cell(text: &str, fg: Rgb, bg: Rgb) -> StyledCell {
    StyledCell {
        text: text.to_owned(),
        style: CellStyle {
            fg,
            bg,
            decoration: Decoration::default(),
            face: FontFace::Regular,
        },
    }
}

/// A row of `text` in the scheme's default colours, `width` cells wide.
fn row(text: &str, width: usize) -> Vec<StyledCell> {
    let mut cells: Vec<StyledCell> = text
        .chars()
        .map(|c| cell(&c.to_string(), LINUX.foreground, LINUX.background))
        .collect();
    cells.resize(width, cell(" ", LINUX.foreground, LINUX.background));
    cells
}

/// A screen of `rows` and the cursor.
fn screen(rows: &[&str], cursor: Option<(u16, u16)>) -> Styled {
    Styled {
        rows: rows.iter().map(|text| row(text, 6)).collect(),
        cursor,
    }
}

/// A writer that takes at most `room` bytes per write, and none while
/// `room` is 0, as a full serial line.
struct Line {
    written: Vec<u8>,
    room: usize,
}

impl Write for Line {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.room == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let taken = bytes.len().min(self.room);
        self.written.extend_from_slice(&bytes[..taken]);
        Ok(taken)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_first_screen_is_written_whole_and_then_only_the_rows_that_changed() {
    let mut view = SerialView::new(
        Line {
            written: Vec::new(),
            room: 4096,
        },
        (6, 3),
        &LINUX,
    );

    view.show(&screen(&["menu", "", "boot"], Some((0, 2))));
    assert_eq!(
        text(&view.out.written),
        "\x1b[?25l\x1b[0m\x1b[H\x1b[2J\x1b[1;1H\x1b[0;39;49mmenu\x1b[0m\x1b[K\
         \x1b[3;1H\x1b[0;39;49mboot\x1b[0m\x1b[K\x1b[3;1H\x1b[?25h"
    );
    view.out.written.clear();
    view.show(&screen(&["menu", "", "BOOT"], None));
    assert_eq!(
        text(&view.out.written),
        "\x1b[?25l\x1b[3;1H\x1b[0;39;49mBOOT\x1b[0m\x1b[K\x1b[0m"
    );
    // The same screen again writes nothing.
    view.out.written.clear();
    view.show(&screen(&["menu", "", "BOOT"], None));
    assert!(view.out.written.is_empty());
    assert!(!view.is_pending());
    // A repaint writes it whole.
    view.repaint();
    view.show(&screen(&["menu", "", "BOOT"], None));
    assert!(text(&view.out.written).contains("\x1b[2J"));
}

#[test]
fn a_full_line_gets_the_latest_screen_once_it_takes_more() {
    let mut view = SerialView::new(
        Line {
            written: Vec::new(),
            room: 0,
        },
        (6, 1),
        &LINUX,
    );
    view.show(&screen(&["one"], None));
    view.show(&screen(&["two"], None));
    assert!(view.is_pending());
    view.out.room = 3;
    view.flush();
    view.flush();
    let written = text(&view.out.written);
    // The first changes, begun, end; then only the latest screen's.
    assert!(written.ends_with("two\x1b[0m\x1b[K\x1b[0m"), "{written:?}");
    assert!(!written.contains("one"), "{written:?}");
    assert!(!view.is_pending());
}

#[test]
fn colours_bold_and_underline_are_ecma_48_s_and_the_screen_is_cut_at_the_terminal_s_size() {
    let mut red = cell("r", LINUX.base[1], LINUX.base[12]);
    red.style.face = FontFace::Bold;
    red.style.decoration = Decoration::underlined(true);
    let wide = cell("日", LINUX.foreground, LINUX.background);
    let spacer = cell("", LINUX.foreground, LINUX.background);
    let styled = Styled {
        rows: vec![
            vec![red, wide.clone(), spacer.clone(), wide, spacer],
            row("second", 6),
            row("third", 6),
        ],
        cursor: Some((5, 2)),
    };
    let mut view = SerialView::new(
        Line {
            written: Vec::new(),
            room: 4096,
        },
        (4, 2),
        &LINUX,
    );

    view.show(&styled);

    let written = text(&view.out.written);
    // Red on bright blue, bold, underlined; a wide character whole; one the
    // edge cuts a space; the third row and the cursor beyond it left out.
    assert!(
        written.contains("\x1b[0;1;4;31;104mr\x1b[0;39;49m日 "),
        "{written:?}"
    );
    assert!(
        written.contains("\x1b[2;1H\x1b[0;39;49msecond")
            || written.contains("\x1b[2;1H\x1b[0;39;49msec"),
        "{written:?}"
    );
    assert!(!written.contains("third"), "{written:?}");
    assert!(written.ends_with("\x1b[0m"), "{written:?}");
    assert_eq!(view.size(), (4, 2));
    // A colour of the 240 beyond is the nearest of the 16.
    assert_eq!(nearest(Rgb(250, 0, 0), &LINUX), 1);
    assert_eq!(nearest(Rgb(255, 90, 90), &LINUX), 9);
}

#[test]
fn keys_are_decoded_and_a_sequence_in_pieces_waits_for_the_rest() {
    let mut input = SerialInput::default();
    let start = Instant::now();
    let keys = input.take(b"a\x1b[", start);
    assert_eq!(keys, [KeyPress::new(Key::Char('a'))]);
    assert_eq!(input.deadline(), Some(start + SERIAL_ESCAPE_WAIT));
    assert!(input.expire(start).is_empty());
    let keys = input.take(b"A", start + Duration::from_millis(10));
    assert_eq!(keys, [KeyPress::new(Key::Up)]);
    assert_eq!(input.deadline(), None);
    // ESC alone, once nothing more came.
    assert!(input.take(b"\x1b", start).is_empty());
    let keys = input.expire(start + SERIAL_ESCAPE_WAIT);
    assert_eq!(keys, [KeyPress::new(Key::Escape)]);
    // CR LF in two reads is one Enter.
    assert_eq!(input.take(b"\r", start), [KeyPress::new(Key::Enter)]);
    assert!(input.take(b"\n", start).is_empty());
    assert_eq!(input.take(b"\n", start), [KeyPress::new(Key::Enter)]);
}

#[test]
fn ctrl_l_asks_for_the_whole_screen() {
    let ctrl_l = KeyPress {
        ctrl: true,
        ..KeyPress::new(Key::Char('l'))
    };
    assert!(has_repaint_key(&[KeyPress::new(Key::Char('x')), ctrl_l]));
    assert!(!has_repaint_key(&[KeyPress::new(Key::Char('l'))]));
}
