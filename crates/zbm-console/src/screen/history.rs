//! The scrollback: what scrolled off the screen, kept for Shift+PgUp and
//! Shift+PgDn in the process's locked memory. While a secret prompt is open
//! there is none, and when it goes, or the console ends, its lines are
//! overwritten first: dropping them alone frees their memory without
//! clearing it.

use alacritty_terminal::grid::{Dimensions as _, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, TermMode};
use zbm_display::keys::{Key, KeyPress};

use super::{Console, config};

impl Console {
    /// The console with a scrollback of `lines`, while no secret prompt is
    /// open.
    #[must_use]
    pub fn with_scrollback(mut self, lines: usize) -> Console {
        self.scrollback = lines;
        self.apply_history();
        self
    }

    /// How many lines of history are kept now.
    pub fn history_lines(&self) -> usize {
        self.term.grid().history_size()
    }

    /// Whether the screen shows history rather than the programs' screen.
    pub fn is_scrolled_back(&self) -> bool {
        self.term.grid().display_offset() > 0
    }

    /// Opens or closes a secret prompt: while one is open, the history is
    /// wiped and none is kept, and the screen shows the programs' screen.
    pub fn set_secret(&mut self, secret: bool) {
        if secret == self.secret {
            return;
        }
        self.secret = secret;
        if secret {
            self.end_seek();
        }
        self.apply_history();
    }

    /// Whether a secret prompt is open.
    pub fn is_secret(&self) -> bool {
        self.secret
    }

    /// Keeps the history the scrollback and a secret prompt allow, the
    /// lines beyond it overwritten.
    fn apply_history(&mut self) {
        let lines = if self.secret { 0 } else { self.scrollback };
        self.term.wipe_history(lines);
        self.term.set_options(Config {
            scrolling_history: lines,
            ..config()
        });
        // The options' events (the title) are not for the programs.
        self.events.0.borrow_mut().clear();
    }

    /// Overwrites all of the history and drops it.
    pub(super) fn wipe_history(&mut self) {
        self.term.wipe_history(0);
        self.term.grid_mut().clear_history();
    }

    /// Takes `press` when it is the console's: Shift+PgUp and Shift+PgDn
    /// scroll the programs' main screen, and the keys of the font's size
    /// (`zoom`), of screenshots (`shot`) and of search and vi mode
    /// (`seek`). Any other key is given back, for the programs.
    pub fn take_key(&mut self, press: KeyPress) -> Option<KeyPress> {
        let press = self.take_zoom_key(press)?;
        let press = self.take_shot_key(press)?;
        let press = self.take_seek_key(press)?;
        let is_main_screen = !self.term.mode().contains(TermMode::ALT_SCREEN);
        let scroll = match press.key {
            Key::PageUp => Scroll::PageUp,
            Key::PageDown => Scroll::PageDown,
            _ => return Some(press),
        };
        if !press.shift || press.ctrl || press.alt || !is_main_screen || self.secret {
            return Some(press);
        }
        self.term.scroll_display(scroll);
        None
    }

    /// Shows the programs' screen again, as typing does.
    pub fn scroll_to_bottom(&mut self) {
        if self.is_scrolled_back() {
            self.term.scroll_display(Scroll::Bottom);
        }
    }

    /// The text of the history and the screen, one line per row, without
    /// trailing spaces or empty lines at the end.
    pub(super) fn scrollback_text(&self) -> String {
        let grid = self.term.grid();
        let mut text = String::new();
        for line in grid.topmost_line().0..=grid.bottommost_line().0 {
            let row = &grid[Line(line)];
            let start = text.len();
            for column in 0..grid.columns() {
                let cell = &row[Column(column)];
                if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    text.push(cell.c);
                    text.extend(cell.zerowidth().unwrap_or_default());
                }
            }
            let end = start + text.get(start..).unwrap_or_default().trim_end().len();
            text.truncate(end);
            text.push('\n');
        }
        let end = text.trim_end().len();
        text.truncate(end);
        text.push('\n');
        text
    }

    /// The screen line that row `row` of the display shows: above the
    /// programs' screen while scrolled back.
    pub(super) fn viewport_line(&self, row: usize) -> i32 {
        let offset = i32::try_from(self.term.grid().display_offset()).unwrap_or(0);
        i32::try_from(row).unwrap_or(i32::MAX) - offset
    }

    /// The rows the display has: the programs' screen's.
    pub(super) fn screen_rows(&self) -> usize {
        self.term.grid().screen_lines()
    }
}
