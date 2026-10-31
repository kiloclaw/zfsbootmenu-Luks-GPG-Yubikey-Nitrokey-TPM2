use std::path::Path;

use pretty_assertions::assert_eq;

use super::*;
use crate::scheme::LINUX;
use crate::screen::styled::StyledCell;

/// A cell of `text` in colours `fg` on `bg` of the Linux palette.
fn cell(text: &str, fg: u8, bg: u8) -> StyledCell {
    StyledCell {
        text: text.to_owned(),
        style: CellStyle {
            fg: LINUX.indexed(fg),
            bg: LINUX.indexed(bg),
            decoration: Decoration::default(),
            face: FontFace::Regular,
        },
    }
}

#[test]
fn the_text_device_is_the_virtual_terminal_s() {
    assert_eq!(device(Path::new("/dev/tty1")), Some("/dev/vcsa1".into()));
    assert_eq!(device(Path::new("/dev/tty12")), Some("/dev/vcsa12".into()));
    for other in ["/dev/tty0", "/dev/tty", "/dev/ttyS0", "/dev/console"] {
        assert_eq!(device(Path::new(other)), None, "{other}");
    }
}

#[test]
fn a_screen_is_the_header_and_each_cell_cut_or_filled_to_the_terminal() {
    let styled = Styled {
        rows: vec![
            vec![cell("a", 1, 0), cell("é", 7, 4), cell("", 7, 0)],
            vec![cell("b", 9, 0)],
            vec![cell("c", 7, 0)],
        ],
        cursor: Some((1, 1)),
    };

    let bytes = frame(&styled, &LINUX, (2, 2), None);

    assert_eq!(bytes[..HEADER], [2, 2, 1, 1]);
    // Red (ANSI 1) is VGA 4; blue (ANSI 4) VGA 1; bright red VGA 12.
    let blank = attribute(
        &CellStyle {
            ..cell(" ", 7, 0).style
        },
        &LINUX,
    );
    assert_eq!(
        bytes[HEADER..],
        [b'a', 0x04, b'?', 0x17, b'b', 0x0c, b' ', blank]
    );
    assert_eq!(bytes.len(), HEADER + 2 * 2 * 2);
}

#[test]
fn an_announcement_is_on_the_last_row_and_the_cursor_stays_inside() {
    let styled = Styled {
        rows: vec![vec![cell("x", 7, 0)]; 3],
        cursor: Some((40, 10)),
    };

    let bytes = frame(&styled, &LINUX, (3, 4), Some("Done: it"));

    assert_eq!(bytes[..HEADER], [3, 4, 3, 2]);
    let last: Vec<u8> = bytes[HEADER + 2 * 4 * 2..]
        .iter()
        .step_by(2)
        .copied()
        .collect();
    assert_eq!(last, b"Done");
    let none = frame(&Styled::default(), &LINUX, (1, 1), None);
    assert_eq!(none[..HEADER], [1, 1, 0, 0]);
    assert_eq!(none[HEADER], b' ');
}
