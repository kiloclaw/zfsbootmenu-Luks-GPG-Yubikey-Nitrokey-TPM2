use pretty_assertions::assert_eq;
use zbm_display::channel::Run;
use zbm_display::frame::Rgb;
use zbm_display::grid::Grid;
use zbm_display::psf::Font;

use super::*;

/// The image's Terminus 8×14.
const TER_V14B: &[u8] = include_bytes!("../../../../../legacy/zfsbootmenu/fonts/ter-v14b.psf");

/// A terminal of 10×3 cells.
fn console() -> Console {
    Console::new(
        Grid::new(Font::parse(TER_V14B).unwrap(), 1),
        (10 * 8, 3 * 14),
    )
}

/// The text of a row.
fn text(row: &[StyledCell]) -> String {
    row.iter().map(|cell| cell.text.as_str()).collect()
}

#[test]
fn the_terminal_s_cells_come_with_their_colours_and_the_cursor() {
    let mut console = console();
    console.feed("ab\x1b[31mc 日\r\n\x1b[0m!".as_bytes());

    let styled = console.styled();

    assert_eq!(styled.rows.len(), 3);
    assert_eq!(text(&styled.rows[0]), "abc 日    ");
    assert_eq!(styled.rows[0][2].style.fg, crate::scheme::LINUX.base[1]);
    // The wide character's second cell is empty.
    assert_eq!(styled.rows[0][5].text, "");
    assert_eq!(styled.cursor, Some((1, 1)));
    console.feed(b"\x1b[?25l");
    assert_eq!(console.styled().cursor, None);
}

#[test]
fn the_client_s_cells_and_the_status_line_are_shown_as_drawn() {
    let mut console = console().with_status_line();
    console.show_cells();
    let style = CellStyle {
        fg: Rgb(1, 2, 3),
        bg: Rgb(0, 0, 0),
        decoration: zbm_display::decor::Decoration::default(),
        face: zbm_display::faces::FontFace::Regular,
    };
    console.put_cells(&[Run {
        column: 0,
        row: 1,
        cells: ["m", "e", "n", "u"].map(str::to_owned).to_vec(),
        style,
    }]);
    console.end_cells_frame(Some((2, 1)));

    let styled = console.styled();

    assert_eq!(styled.rows.len(), 3);
    assert_eq!(text(&styled.rows[1]).trim_end(), "menu");
    assert_eq!(styled.cursor, Some((2, 1)));
    // The status line, which says nothing yet.
    assert!(
        styled.rows[2]
            .iter()
            .all(|cell| cell.style.bg == crate::scheme::LINUX.foreground)
    );
}
