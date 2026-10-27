//! The text of the screen as shown, for the mirror of the VM tests,
//! screenshots and the text the console leaves on the kernel console.

use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;

use super::Console;

impl Console {
    /// The text of the screen as shown, one line per row, without trailing
    /// spaces, combining marks after their characters; the status line
    /// last, if there is one. It is built in one string, measured first and
    /// made that large at once: growing it would leave a partial copy of the
    /// screen in freed memory.
    pub fn text(&self) -> String {
        let mut length = 0;
        self.shown_chars(|c| length += c.map_or(1, char::len_utf8));
        let mut text = String::with_capacity(length);
        self.shown_chars(|c| match c {
            Some(c) => text.push(c),
            None => end_line(&mut text),
        });
        text
    }

    /// Gives `each` the characters of the screen as shown, row after row,
    /// combining marks after their characters, and `None` at the end of each
    /// row; then the status line's, if there is one.
    fn shown_chars(&self, mut each: impl FnMut(Option<char>)) {
        let grid = self.term.grid();
        let size = (grid.screen_lines(), grid.columns());
        let overlay = self.overlay();
        for line in 0..grid.screen_lines() {
            let row = &grid[Line(self.viewport_line(line))];
            for column in 0..grid.columns() {
                let cell = &row[Column(column)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                if let Some(c) = overlay.char_at((line, column), size) {
                    each(Some(c));
                } else {
                    each(Some(cell.c));
                    for &mark in cell.zerowidth().unwrap_or_default() {
                        each(Some(mark));
                    }
                }
            }
            each(None);
        }
        if let Some(status) = self.status_text() {
            for &c in status {
                each(Some(c));
            }
            each(None);
        }
    }
}

/// Ends the last line of `text`: its trailing spaces go, a newline comes.
fn end_line(text: &mut String) {
    let line = text.rfind('\n').map_or(0, |end| end + 1);
    let kept = text.get(line..).unwrap_or_default().trim_end().len();
    text.truncate(line + kept);
    text.push('\n');
}
