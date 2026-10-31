//! The terminal: Alacritty's state machine fed with what the programs
//! write, its cells drawn as pixels with the image's console font.
//!
//! Only what changed is drawn again (Alacritty's damage), a scrolled
//! screen is moved rather than drawn (`scroll`), and only the area drawn
//! reaches the display (`Frame::take_damage`), so that a firmware
//! framebuffer, which is slow to write, keeps up with a shell.
//!
//! What scrolls off the screen is kept for Shift+PgUp only as far as the
//! scrollback allows (`history`), and none while a secret prompt is open,
//! so that a passphrase shown by mistake does not stay in memory; the
//! answers the terminal owes the programs (cursor position, device
//! attributes, colours) are kept apart for the caller to write back. When
//! the terminal is dropped, its cells, its history and the pixels drawn
//! from them are wiped.

use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::rc::Rc;
use std::time::Instant;

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, Processor};
use zbm_display::faces::FontFace;
use zbm_display::frame::{Frame, Rgb};
use zbm_display::grid::{CellStyle, Grid, Neighbour, Text};
use zbm_display::render::{CellRenderer, warm_chars};

use crate::colour::{self, Shown, Table};
use crate::keys::{self, Flavour, Modes};
use crate::scheme::{LINUX, Scheme};
use crate::typing::Preview;
pub use bell::Ring;
pub use blink::BLINK;
pub use config::NAME;
use config::config;
use look::{draw_cursor, look};
use scroll::Shift;
pub use status::{Clock, Locks, Status};

mod bell;
mod blink;
mod cells;
mod config;
mod events;
mod history;
mod look;
mod place;
mod scroll;
pub(super) mod seek;
mod shot;
mod status;
pub mod styled;
mod text;
pub mod zoom;

/// The events of the terminal, kept until the console handles them.
#[derive(Clone, Default)]
struct Events(Rc<RefCell<Vec<Event>>>);

impl EventListener for Events {
    fn send_event(&self, event: Event) {
        self.0.borrow_mut().push(event);
    }
}

/// The terminal on the display.
pub struct Console {
    term: Term<Events>,
    parser: Processor,
    events: Events,
    /// The font's grid, through the cache of drawn cells.
    grid: CellRenderer,
    frame: Frame,
    columns: u16,
    rows: u16,
    replies: Vec<u8>,
    blink: blink::Cursor,
    /// The blinking of text (SGR 5 and 6).
    text_blink: blink::Text,
    /// A hash of each row as drawn, `None` where it is not known.
    drawn_rows: Vec<Option<u64>>,
    /// The terminal description the programs read keys through.
    flavour: Flavour,
    /// What is being typed, shown at the cursor.
    preview: Option<Preview>,
    /// Whether the screen must be drawn again as a whole: what it shows
    /// besides the terminal changed.
    redraw: bool,
    /// The lines of history kept while no secret prompt is open.
    scrollback: usize,
    /// Whether a secret prompt is open.
    secret: bool,
    /// Search and vi mode.
    seek: seek::Seek,
    /// What was drawn over the screen.
    overlay: seek::Overlay,
    /// The change of the font's size asked for with the keys.
    zoom: Option<zoom::Zoom>,
    /// The colour scheme.
    scheme: &'static Scheme,
    /// The title the programs set.
    title: Option<String>,
    /// The bell.
    bell: bell::Bell,
    /// The status line, when the console has one.
    status_line: Option<Vec<char>>,
    /// When the status line must be said again.
    status_at: Option<Instant>,
    /// What the status line says now: the status to the minute, the title
    /// and the width it was made for, so that the same is not made again on
    /// every pass of the loop.
    status_said: Option<(Status, Option<String>, u16)>,
    /// Whether Print Screen asked for a screenshot.
    is_shot: bool,
    /// The lines Alacritty says changed, kept so that drawing a frame
    /// allocates nothing.
    damaged: Vec<(usize, usize, usize)>,
    /// The hashes of the rows to draw, kept for the same reason.
    hashes: Vec<u64>,
    /// How long without a key blanks the screen (C64).
    blank_after: Option<std::time::Duration>,
    /// The cells of the client of the channel, while they are shown.
    cells: Option<cells::Surface>,
    /// The size the screen is held to, centred on the display, while a
    /// smaller terminal is the one typed on: the serial console's
    /// (ROADMAP.md UI9).
    held: Option<(u16, u16)>,
}

/// The cursor as drawn: where, its shape (hidden while it blinks off),
/// and what is being typed, shown from there.
#[derive(Clone, Copy, Debug)]
struct Cursor {
    line: usize,
    column: usize,
    shape: CursorShape,
    preview: Option<Preview>,
}

impl std::fmt::Debug for Console {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Console")
            .field("columns", &self.columns)
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

impl Console {
    /// A blank terminal on a black frame of `pixels` (width, height), in as
    /// many cells of `grid`'s font as fit, at least one.
    pub fn new(grid: Grid, pixels: (u32, u32)) -> Console {
        let (columns, rows) = grid.dimensions(pixels);
        let (columns, rows) = (columns.max(1), rows.max(1));
        let events = Events::default();
        let size = TermSize::new(usize::from(columns), usize::from(rows));
        let mut console = Console {
            term: Term::new(config(), &size, events.clone()),
            parser: Processor::new(),
            events,
            grid: CellRenderer::new(grid),
            frame: Frame::new(pixels.0, pixels.1),
            columns,
            rows,
            replies: Vec::new(),
            blink: blink::Cursor::default(),
            text_blink: blink::Text::default(),
            drawn_rows: vec![None; usize::from(rows)],
            flavour: Flavour::Linux,
            preview: None,
            redraw: false,
            scrollback: 0,
            secret: false,
            seek: seek::Seek::default(),
            overlay: seek::Overlay::default(),
            zoom: None,
            scheme: &LINUX,
            title: None,
            bell: bell::Bell::default(),
            status_line: None,
            status_at: None,
            status_said: None,
            is_shot: false,
            damaged: Vec::new(),
            hashes: Vec::new(),
            blank_after: None,
            cells: None,
            held: None,
        };
        console.warm();
        console.draw();
        console
    }

    /// Draws ASCII and the lines of boxes in the screen's colours into the
    /// cache of cells, so that the first frames only copy them.
    fn warm(&mut self) {
        let style = CellStyle {
            fg: self.scheme.foreground,
            bg: self.scheme.background,
            decoration: zbm_display::decor::Decoration::default(),
            face: FontFace::Regular,
        };
        self.grid.warm(warm_chars(), style);
    }

    /// The font's grid the cells are drawn with.
    pub fn grid(&self) -> &Grid {
        self.grid.grid()
    }

    /// The size of the frame in pixels: width and height.
    pub fn pixels(&self) -> (u32, u32) {
        self.frame.size()
    }

    /// The size in cells: columns and rows.
    pub fn size(&self) -> (u16, u16) {
        (self.columns, self.rows)
    }

    /// The size the programs are told, in cells and in pixels per cell.
    pub fn window_size(&self) -> WindowSize {
        let (width, height) = self.grid.cell();
        WindowSize {
            num_lines: self.rows,
            num_cols: self.columns,
            cell_width: u16::try_from(width).unwrap_or(u16::MAX),
            cell_height: u16::try_from(height).unwrap_or(u16::MAX),
        }
    }

    /// Takes what the programs wrote.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
        self.blink.fed = true;
        self.forget_match();
        self.answer();
    }

    /// When the console must be woken: a synchronized update ends, or the
    /// cursor blinks.
    pub fn deadline(&self) -> Option<Instant> {
        let sync = self.parser.sync_timeout().sync_timeout();
        [
            sync,
            self.blink.at,
            self.text_blink.at(),
            self.bell.at(),
            self.status_at(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Ends a synchronized update whose time ran out at `now`, and blinks
    /// the cursor and the text when their time came.
    pub fn tick(&mut self, now: Instant) {
        let sync = self.parser.sync_timeout().sync_timeout();
        if sync.is_some_and(|deadline| deadline <= now) {
            self.parser.stop_sync(&mut self.term);
            self.answer();
        }
        // The cursor blinks while a blinking cursor is shown.
        let shape = self.term.renderable_content().cursor.shape;
        let is_blinking = shape != CursorShape::Hidden && self.term.cursor_style().blinking;
        self.blink.time(is_blinking, now);
        self.text_blink.tick(now);
        self.tick_bell(now);
    }

    /// Has the keys sent as the terminal description `flavour` describes
    /// them, which the programs run with.
    #[must_use]
    pub fn with_flavour(mut self, flavour: Flavour) -> Console {
        self.flavour = flavour;
        self
    }

    /// What the terminal's modes ask of the keys: the cursor keys
    /// (DECCKM), the keypad (DECKPAM), Enter (LNM) and the kitty keyboard
    /// protocol.
    pub fn key_modes(&self) -> Modes {
        let mode = *self.term.mode();
        let kitty = [
            (TermMode::DISAMBIGUATE_ESC_CODES, keys::DISAMBIGUATE),
            (TermMode::REPORT_ALL_KEYS_AS_ESC, keys::ALL_KEYS),
            (TermMode::REPORT_ASSOCIATED_TEXT, keys::TEXT),
        ]
        .into_iter()
        .filter(|(flag, _)| mode.contains(*flag))
        .fold(0, |flags, (_, bit)| flags | bit);
        Modes {
            flavour: self.flavour,
            application_cursor: mode.contains(TermMode::APP_CURSOR),
            application_keypad: mode.contains(TermMode::APP_KEYPAD),
            new_line: mode.contains(TermMode::LINE_FEED_NEW_LINE),
            kitty,
        }
    }

    /// The frame drawn on.
    pub fn frame_mut(&mut self) -> &mut Frame {
        &mut self.frame
    }

    /// Draws what changed since the last call into the frame; returns how
    /// many cells were drawn, a scroll counting as one.
    /// Blinking text shown or hidden draws the screen again, which draws
    /// the rows that changed.
    pub fn draw(&mut self) -> usize {
        if self.cells.is_some() {
            return self.draw_cells();
        }
        let mut lines = std::mem::take(&mut self.damaged);
        lines.clear();
        let is_full = match self.term.damage() {
            TermDamage::Full => true,
            TermDamage::Partial(damage) => {
                lines.extend(damage.map(|bounds| (bounds.line, bounds.left, bounds.right)));
                false
            }
        };
        self.term.reset_damage();
        let cursor = self.drawn_cursor();
        let overlay = self.overlay();
        let is_new_overlay = overlay != self.overlay;
        self.overlay = overlay;
        let is_whole =
            self.text_blink.has_turned() | std::mem::take(&mut self.redraw) | is_new_overlay;
        let drawn = if is_full || is_whole {
            self.draw_screen(cursor)
        } else {
            self.draw_lines(&lines, cursor)
        } + self.draw_edges(cursor)
            + self.draw_flash();
        self.damaged = lines;
        self.text_blink.follow(self.has_blinking_text());
        drawn
    }

    /// Whether the screen shows blinking text (SGR 5 or 6).
    fn has_blinking_text(&self) -> bool {
        let grid = self.term.grid();
        (0..grid.screen_lines()).any(|line| {
            let row = &grid[Line(i32::try_from(line).unwrap_or(i32::MAX))];
            (0..grid.columns()).any(|column| row[Column(column)].flags.contains(Flags::BLINK))
        })
    }

    /// How the screen is shown now: reversed or not (DECSCNM), blinking
    /// text shown or hidden.
    fn shown(&self) -> Shown {
        Shown {
            reverse: self.term.mode().contains(TermMode::REVERSE_SCREEN),
            blink_off: self.text_blink.is_off(),
            scheme: self.scheme,
        }
    }

    /// What is shown at the cursor as it is typed.
    pub fn preview(&self) -> Option<Preview> {
        self.preview
    }

    /// Shows `preview` at the cursor, until it is `None`.
    pub fn set_preview(&mut self, preview: Option<Preview>) {
        if self.preview != preview {
            self.preview = preview;
            self.redraw = true;
        }
    }

    /// Where the cursor is drawn, and its shape: nowhere while it is
    /// hidden, or blinked off, unless something is being typed there.
    fn drawn_cursor(&self) -> Option<Cursor> {
        let cursor = self.term.renderable_content().cursor;
        // On the display, below the history shown while scrolled back.
        let offset = i32::try_from(self.term.grid().display_offset()).unwrap_or(0);
        let line = usize::try_from(cursor.point.line.0 + offset)
            .ok()
            .filter(|&line| line < self.screen_rows())?;
        let is_shown = cursor.shape != CursorShape::Hidden && !self.blink.off;
        (is_shown || self.preview.is_some()).then_some(Cursor {
            line,
            column: cursor.point.column.0,
            shape: if is_shown {
                cursor.shape
            } else {
                CursorShape::Hidden
            },
            preview: self.preview,
        })
    }

    /// Draws the cells of `lines` (line, first and last column), and keeps
    /// the hashes of their rows. The cells on either side are drawn again
    /// too: what a glyph of the changed cells reaches into them changed.
    fn draw_lines(&mut self, lines: &[(usize, usize, usize)], cursor: Option<Cursor>) -> usize {
        let mut drawn = 0;
        let last = usize::from(self.columns) - 1;
        for &(line, left, right) in lines {
            for column in left.saturating_sub(1)..=right.saturating_add(1).min(last) {
                self.draw_cell(line, column, cursor);
                drawn += 1;
            }
            let hash = self.row_hash(line, cursor);
            if let Some(row) = self.drawn_rows.get_mut(line) {
                *row = Some(hash);
            }
        }
        drawn
    }

    /// Draws the whole screen, which changed as a whole: the rows drawn are
    /// moved first when it scrolled, then the rows that differ are drawn.
    fn draw_screen(&mut self, cursor: Option<Cursor>) -> usize {
        let rows = usize::from(self.display_rows());
        let mut next = std::mem::take(&mut self.hashes);
        next.clear();
        next.extend((0..rows).map(|line| self.row_hash(line, cursor)));
        let shift = scroll::best_shift(&self.drawn_rows, &next);
        let (_, cell) = self.grid.cell();
        let top = self.grid.grid().origin().1;
        let pixels = |rows: usize| u32::try_from(rows).unwrap_or(u32::MAX).saturating_mul(cell);
        match shift {
            Shift::None => {}
            Shift::Up(by) => self.frame.shift_rows(top, pixels(rows), pixels(by), true),
            Shift::Down(by) => self.frame.shift_rows(top, pixels(rows), pixels(by), false),
        }
        scroll::shift_hashes(&mut self.drawn_rows, shift);
        let mut drawn = usize::from(shift != Shift::None);
        for (line, &hash) in next.iter().enumerate() {
            if self.drawn_rows.get(line) == Some(&Some(hash)) {
                continue;
            }
            for column in 0..usize::from(self.columns) {
                self.draw_cell(line, column, cursor);
                drawn += 1;
            }
            if let Some(row) = self.drawn_rows.get_mut(line) {
                *row = Some(hash);
            }
        }
        self.hashes = next;
        drawn
    }

    /// A hash of the row at `line` as it is drawn, the cursor included.
    fn row_hash(&self, line: usize, cursor: Option<Cursor>) -> u64 {
        let mut hasher = DefaultHasher::new();
        let shown = self.shown();
        for column in 0..usize::from(self.columns) {
            look(&self.term, (line, column), cursor, shown, &self.overlay).hash(&mut hasher);
        }
        hasher.finish()
    }

    /// The flags of the cell at `line`, `column`, if there is one.
    fn flags(&self, line: usize, column: usize) -> Option<Flags> {
        let grid = self.term.grid();
        let row = self.viewport_line(line);
        (line < grid.screen_lines() && column < grid.columns())
            .then(|| grid[Line(row)][Column(column)].flags)
    }

    /// The characters next to the cells from `column`, `span` wide, whose
    /// glyphs may reach into them: the one before, from the start of its
    /// cells, and the one after; as an array and how many of it there are,
    /// so that drawing a cell allocates nothing.
    fn neighbours(&self, line: usize, column: usize, span: usize) -> ([Neighbour; 2], usize) {
        let (cell, _) = self.grid.cell();
        let width = i64::from(cell);
        let none = Neighbour {
            c: ' ',
            fg: Rgb::BLACK,
            face: FontFace::Regular,
            offset: 0,
        };
        let mut found = [none; 2];
        let mut count = 0;
        let before = column.checked_sub(1).map(|left| {
            let is_spacer = self
                .flags(line, left)
                .is_some_and(|flags| flags.contains(Flags::WIDE_CHAR_SPACER));
            if is_spacer {
                (left.saturating_sub(1), -2 * width)
            } else {
                (left, -width)
            }
        });
        let after = Some((column + span, width * i64::try_from(span).unwrap_or(1)));
        for (at, offset) in before.into_iter().chain(after) {
            if let Some((c, style, ..)) =
                look(&self.term, (line, at), None, self.shown(), &self.overlay)
                && c != ' '
                && let Some(slot) = found.get_mut(count)
            {
                *slot = Neighbour {
                    c,
                    fg: style.fg,
                    face: style.face,
                    offset,
                };
                count += 1;
            }
        }
        (found, count)
    }

    /// Draws the cell at `line`, `column`, with the cursor over it when it
    /// is there. A wide character is drawn over both of its cells, from
    /// either.
    fn draw_cell(&mut self, line: usize, column: usize, cursor: Option<Cursor>) {
        let is_spacer = |flags: Option<Flags>| {
            flags.is_some_and(|flags| flags.contains(Flags::WIDE_CHAR_SPACER))
        };
        let column = if column > 0 && is_spacer(self.flags(line, column)) {
            column - 1
        } else {
            column
        };
        let (Ok(x), Ok(y)) = (u16::try_from(column), u16::try_from(line)) else {
            return;
        };
        let shown = self.shown();
        let Some((c, style, marks, shape)) =
            look(&self.term, (line, column), cursor, shown, &self.overlay)
        else {
            return;
        };
        let is_wide = self
            .flags(line, column)
            .is_some_and(|flags| flags.contains(Flags::WIDE_CHAR));
        let span = if is_wide { 2 } else { 1 };
        let (neighbours, count) = self.neighbours(line, column, usize::from(span));
        let text = Text { c, style, span };
        let neighbours = neighbours.get(..count).unwrap_or_default();
        self.grid.draw_text(&mut self.frame, x, y, text, neighbours);
        // Combining marks, such as the accent of an `e` followed by U+0301,
        // are drawn over the character they belong to.
        for &mark in marks {
            self.grid.draw_mark(&mut self.frame, x, y, mark, style.fg);
        }
        if let Some(shape) = shape {
            let area = self.grid.area(x, y);
            let colours = Table::new(self.term.colors(), self.scheme);
            let colour = colour::cursor_colour(colours).unwrap_or(style.fg);
            draw_cursor(&mut self.frame, area, shape, colour);
        }
    }

    /// Wipes what the terminal shows and the pixels drawn from it: the
    /// screen is cleared, its cells reset, the frame painted black.
    pub fn wipe(&mut self) {
        self.cells = None;
        // Clearing the screen moves it into the history, which goes after.
        self.feed(b"\x1b[0m\x1b[H\x1b[2J");
        self.wipe_history();
        self.seek = seek::Seek::default();
        zbm_sys::wipe_vec(&mut self.replies);
        self.drawn_rows.fill(None);
        let (width, height) = self.frame.size();
        self.frame.fill(
            zbm_display::frame::Area {
                x: 0,
                y: 0,
                width,
                height,
            },
            zbm_display::frame::Rgb::BLACK,
        );
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        self.wipe();
    }
}

#[cfg(test)]
mod tests;
