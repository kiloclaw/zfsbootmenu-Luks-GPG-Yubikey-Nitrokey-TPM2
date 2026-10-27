//! Search and vi mode in the scrollback, with Alacritty's vi mode.
//!
//! Ctrl+Shift+Space starts vi mode: the keys move a cursor over the screen
//! and the history, as in vi, and none reach the programs. Ctrl+Shift+F and
//! Ctrl+Shift+B, or `/` and `?` in vi mode, search down and up for the text
//! typed, on a bar over the last row; `n` and `N` find the next match and
//! the one before. Escape ends each. Ctrl+Shift+S saves the scrollback,
//! for `zbm report`. None of them is open at a secret prompt or on the
//! alternate screen.

use alacritty_terminal::grid::{Dimensions as _, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::search::{Match, RegexSearch};
use alacritty_terminal::vi_mode::ViMotion;
use zbm_display::keys::{Key, KeyPress};

use super::Console;

/// The search bar, open while its text is typed.
#[derive(Debug)]
struct Bar {
    /// The text searched for.
    text: Vec<char>,
    /// Down (right) or up (left).
    direction: Direction,
    /// Where the vi cursor was when the bar opened.
    origin: Point,
    /// How far the screen was scrolled back then.
    offset: usize,
    /// Whether vi mode was on then.
    was_vi: bool,
}

/// The state of search and vi mode.
#[derive(Debug, Default)]
pub(super) struct Seek {
    bar: Option<Bar>,
    /// The last search, for `n` and `N`.
    last: Option<(RegexSearch, Direction)>,
    /// The match shown.
    found: Option<Match>,
    /// Whether the scrollback must be saved.
    save: bool,
    /// What is said on the bar until the next key: where the scrollback
    /// was saved.
    notice: Option<Vec<char>>,
}

/// What is drawn over the screen: the search bar on the last row, where
/// the screen is in the history on the first, and the match found.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Overlay {
    pub bar: Option<Vec<char>>,
    pub status: Option<Vec<char>>,
    pub label: Option<Vec<char>>,
    pub found: Option<Match>,
}

impl Overlay {
    /// Whether `point` of the grid is in the match found.
    pub fn has_found(&self, point: Point) -> bool {
        self.found
            .as_ref()
            .is_some_and(|found| found.contains(&point))
    }

    /// What the overlay draws at (row, column) of `rows` and `columns`,
    /// if anything.
    pub fn char_at(
        &self,
        (row, column): (usize, usize),
        (rows, columns): (usize, usize),
    ) -> Option<char> {
        if row == rows
            && let Some(status) = &self.status
        {
            return Some(status.get(column).copied().unwrap_or(' '));
        }
        if row + 1 == rows
            && let Some(bar) = &self.bar
        {
            return Some(bar.get(column).copied().unwrap_or(' '));
        }
        let label = self.label.as_ref().filter(|_| row == 0)?;
        let start = columns.checked_sub(label.len())?;
        label.get(column.checked_sub(start)?).copied()
    }
}

/// `text` as a regular expression that matches it as it is.
pub(super) fn literal(text: &[char]) -> String {
    let mut pattern = String::with_capacity(text.len() * 2);
    for &c in text {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern
}

/// The vi motion of `c`, as in vi.
fn motion(c: char) -> Option<ViMotion> {
    Some(match c {
        'h' => ViMotion::Left,
        'j' => ViMotion::Down,
        'k' => ViMotion::Up,
        'l' => ViMotion::Right,
        '0' => ViMotion::First,
        '$' => ViMotion::Last,
        '^' => ViMotion::FirstOccupied,
        'H' => ViMotion::High,
        'M' => ViMotion::Middle,
        'L' => ViMotion::Low,
        'w' => ViMotion::SemanticRight,
        'b' => ViMotion::SemanticLeft,
        'e' => ViMotion::SemanticRightEnd,
        'W' => ViMotion::WordRight,
        'B' => ViMotion::WordLeft,
        'E' => ViMotion::WordRightEnd,
        '%' => ViMotion::Bracket,
        '{' => ViMotion::ParagraphUp,
        '}' => ViMotion::ParagraphDown,
        _ => return None,
    })
}

/// The vi motion of a key that is not a character.
fn key_motion(key: Key) -> Option<ViMotion> {
    Some(match key {
        Key::Left => ViMotion::Left,
        Key::Down => ViMotion::Down,
        Key::Up => ViMotion::Up,
        Key::Right => ViMotion::Right,
        Key::Home => ViMotion::First,
        Key::End => ViMotion::Last,
        _ => return None,
    })
}

/// Whether `press` is Ctrl+Shift with `c`, in either case.
fn is_ctrl_shift(press: KeyPress, c: char) -> bool {
    press.ctrl
        && press.shift
        && !press.alt
        && matches!(press.key, Key::Char(k) if k.eq_ignore_ascii_case(&c))
}

/// The opposite of `direction`.
fn opposite(direction: Direction) -> Direction {
    match direction {
        Direction::Left => Direction::Right,
        Direction::Right => Direction::Left,
    }
}

impl Console {
    /// Whether vi mode is on.
    pub fn is_vi_mode(&self) -> bool {
        self.term.mode().contains(TermMode::VI)
    }

    /// Whether the search bar is open.
    pub fn is_searching(&self) -> bool {
        self.seek.bar.is_some()
    }

    /// Takes `press` when search or vi mode is open, or starts them;
    /// `Some(press)` when it is not theirs.
    pub(super) fn take_seek_key(&mut self, press: KeyPress) -> Option<KeyPress> {
        self.seek.notice = None;
        let is_closed = self.secret || self.term.mode().contains(TermMode::ALT_SCREEN);
        if self.seek.bar.is_some() {
            self.bar_key(press);
        } else if is_ctrl_shift(press, 's') && !is_closed {
            self.seek.save = true;
        } else if self.is_vi_mode() {
            self.vi_key(press);
        } else if is_closed {
            return Some(press);
        } else if is_ctrl_shift(press, ' ') {
            self.term.toggle_vi_mode();
        } else if is_ctrl_shift(press, 'f') {
            self.open_bar(Direction::Right);
        } else if is_ctrl_shift(press, 'b') {
            self.open_bar(Direction::Left);
        } else {
            return Some(press);
        }
        // The terminal's events of vi mode are not for the programs.
        self.events.0.borrow_mut().clear();
        None
    }

    /// Ends search and vi mode, and shows the programs' screen.
    pub(super) fn end_seek(&mut self) {
        self.seek = Seek::default();
        if self.is_vi_mode() {
            self.term.toggle_vi_mode();
        }
        self.term.scroll_display(Scroll::Bottom);
        self.events.0.borrow_mut().clear();
    }

    /// The text of the scrollback and the screen when Ctrl+Shift+S asked
    /// for it to be saved, once.
    pub fn take_save(&mut self) -> Option<String> {
        std::mem::take(&mut self.seek.save).then(|| self.scrollback_text())
    }

    /// Says `notice` on the bar, until the next key.
    pub fn set_notice(&mut self, notice: &str) {
        self.seek.notice = Some(notice.chars().collect());
    }

    /// Forgets the match shown: what the programs write moves it.
    pub(super) fn forget_match(&mut self) {
        self.seek.found = None;
    }

    /// What is drawn over the screen now.
    pub(super) fn overlay(&self) -> Overlay {
        let bar = self.seek.bar.as_ref().map(|bar| {
            let mut text: Vec<char> = match bar.direction {
                Direction::Right => "Search down: ",
                Direction::Left => "Search up: ",
            }
            .chars()
            .collect();
            text.extend_from_slice(&bar.text);
            text.push('_');
            if !bar.text.is_empty() && self.seek.found.is_none() {
                text.extend(" (not found)".chars());
            }
            text
        });
        let bar = bar.or_else(|| self.seek.notice.clone());
        let grid = self.term.grid();
        let label = (self.is_vi_mode() || self.is_scrolled_back()).then(|| {
            format!("[{}/{}]", grid.display_offset(), grid.history_size())
                .chars()
                .collect()
        });
        Overlay {
            bar,
            status: self.status_text().map(<[char]>::to_vec),
            label,
            found: self.seek.found.clone(),
        }
    }

    /// Opens the search bar, searching in `direction` from the vi cursor;
    /// vi mode starts with it.
    fn open_bar(&mut self, direction: Direction) {
        let was_vi = self.is_vi_mode();
        if !was_vi {
            self.term.toggle_vi_mode();
        }
        self.seek.bar = Some(Bar {
            text: Vec::new(),
            direction,
            origin: self.term.vi_mode_cursor.point,
            offset: self.term.grid().display_offset(),
            was_vi,
        });
        self.seek.found = None;
    }

    /// A key typed into the search bar.
    fn bar_key(&mut self, press: KeyPress) {
        let Some(bar) = self.seek.bar.as_mut() else {
            return;
        };
        match press.key {
            Key::Char(c) if !press.ctrl && !press.alt => bar.text.push(c),
            Key::Backspace => {
                bar.text.pop();
            }
            Key::Enter => {
                self.seek.bar = None;
                return;
            }
            Key::Escape => {
                self.close_bar();
                return;
            }
            _ => return,
        }
        self.search_from_origin();
    }

    /// Closes the search bar and goes back to where it opened.
    fn close_bar(&mut self) {
        let Some(bar) = self.seek.bar.take() else {
            return;
        };
        self.seek.found = None;
        if bar.was_vi {
            self.term.vi_goto_point(bar.origin);
            let offset = self.term.grid().display_offset();
            let delta = i32::try_from(bar.offset).unwrap_or(0) - i32::try_from(offset).unwrap_or(0);
            self.term.scroll_display(Scroll::Delta(delta));
        } else {
            self.end_seek();
        }
    }

    /// Searches for the bar's text from where the bar opened.
    fn search_from_origin(&mut self) {
        let Some(bar) = &self.seek.bar else {
            return;
        };
        let (origin, direction) = (bar.origin, bar.direction);
        self.seek.found = None;
        self.seek.last = None;
        if bar.text.is_empty() {
            self.term.vi_goto_point(origin);
            return;
        }
        let Ok(mut regex) = RegexSearch::new(&literal(&bar.text)) else {
            return;
        };
        let found = self
            .term
            .search_next(&mut regex, origin, direction, Side::Left, None);
        self.seek.last = Some((regex, direction));
        self.show_match(found);
    }

    /// Goes to the next match of the last search, or the one before.
    fn search_again(&mut self, is_back: bool) {
        let Some((mut regex, direction)) = self.seek.last.take() else {
            return;
        };
        let direction_now = if is_back {
            opposite(direction)
        } else {
            direction
        };
        let point = self.term.vi_mode_cursor.point;
        let origin = match direction_now {
            Direction::Right => point.add(&self.term, Boundary::None, 1),
            Direction::Left => point.sub(&self.term, Boundary::None, 1),
        };
        let found = self
            .term
            .search_next(&mut regex, origin, direction_now, Side::Left, None);
        self.seek.last = Some((regex, direction));
        self.show_match(found);
    }

    /// Moves the vi cursor to `found`, which is shown, if there is one.
    fn show_match(&mut self, found: Option<Match>) {
        if let Some(found) = &found {
            self.term.vi_goto_point(*found.start());
        }
        self.seek.found = found;
    }

    /// A key in vi mode.
    fn vi_key(&mut self, press: KeyPress) {
        if is_ctrl_shift(press, ' ') {
            self.end_seek();
            return;
        }
        let rows = i32::try_from(self.term.grid().screen_lines()).unwrap_or(1);
        if press.ctrl {
            match press.key {
                Key::Char('c' | 'C') => self.end_seek(),
                Key::Char('b' | 'B') => self.vi_scroll(rows),
                Key::Char('f' | 'F') => self.vi_scroll(-rows),
                Key::Char('u' | 'U') => self.vi_scroll(rows / 2),
                Key::Char('d' | 'D') => self.vi_scroll(-rows / 2),
                _ => {}
            }
            return;
        }
        if press.alt {
            return;
        }
        match press.key {
            Key::Escape | Key::Char('q' | 'i') => self.end_seek(),
            Key::PageUp => self.vi_scroll(rows),
            Key::PageDown => self.vi_scroll(-rows),
            Key::Char('g') => self.vi_goto_line(self.term.topmost_line()),
            Key::Char('G') => self.vi_goto_line(self.term.bottommost_line()),
            Key::Char('/') => self.open_bar(Direction::Right),
            Key::Char('?') => self.open_bar(Direction::Left),
            Key::Char('n') => self.search_again(false),
            Key::Char('N') => self.search_again(true),
            Key::Char(c) => {
                if let Some(motion) = motion(c) {
                    self.term.vi_motion(motion);
                }
            }
            key => {
                if let Some(motion) = key_motion(key) {
                    self.term.vi_motion(motion);
                }
            }
        }
    }

    /// Scrolls `lines` up (down when negative) in vi mode, the cursor
    /// moved with the screen.
    fn vi_scroll(&mut self, lines: i32) {
        let cursor = self.term.vi_mode_cursor.scroll(&self.term, lines);
        self.term.scroll_display(Scroll::Delta(lines));
        self.term.vi_goto_point(cursor.point);
    }

    /// Moves the vi cursor to the start of `line`.
    fn vi_goto_line(&mut self, line: Line) {
        self.term.vi_goto_point(Point::new(line, Column(0)));
    }
}
