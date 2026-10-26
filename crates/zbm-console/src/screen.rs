//! The terminal: Alacritty's state machine fed with what the programs
//! write, its cells drawn as pixels with the image's console font.
//!
//! Only what changed is drawn again (Alacritty's damage), a scrolled
//! screen is moved rather than drawn (`scroll`), and only the area drawn
//! reaches the display (`Frame::take_damage`), so that a firmware
//! framebuffer, which is slow to write, keeps up with a shell.
//!
//! There is no scrollback: what scrolls off the screen is gone, so a
//! passphrase shown by mistake does not stay in memory, and the answers the
//! terminal owes the programs (cursor position, device attributes, colours)
//! are kept apart for the caller to write back. When the terminal is
//! dropped, its cells and the pixels drawn from them are wiped.

use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::rc::Rc;
use std::time::{Duration, Instant};

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Osc52, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle, Processor, Rgb as TermRgb};
use zbm_display::frame::{Area, Frame, Rgb};
use zbm_display::grid::{CellStyle, Grid};

use crate::colour;
use scroll::Shift;

mod scroll;

/// The events of the terminal, kept until the console handles them.
#[derive(Clone, Default)]
struct Events(Rc<RefCell<Vec<Event>>>);

impl EventListener for Events {
    fn send_event(&self, event: Event) {
        self.0.borrow_mut().push(event);
    }
}

/// How long a blinking cursor is shown, and then hidden.
pub const BLINK: Duration = Duration::from_millis(500);

/// The blinking of the cursor: a timer that runs only while a blinking
/// cursor is shown.
#[derive(Debug, Default)]
struct Blink {
    /// When the cursor is shown or hidden next.
    at: Option<Instant>,
    /// Whether it is hidden now.
    off: bool,
    /// Whether the programs wrote since the last tick: the cursor is shown
    /// again, as it is while they type.
    fed: bool,
}

/// The terminal on the display.
pub struct Console {
    term: Term<Events>,
    parser: Processor,
    events: Events,
    grid: Grid,
    frame: Frame,
    columns: u16,
    rows: u16,
    replies: Vec<u8>,
    blink: Blink,
    /// A hash of each row as drawn, `None` where it is not known.
    drawn_rows: Vec<Option<u64>>,
}

/// The cursor as drawn: where, and its shape.
#[derive(Clone, Copy, Debug)]
struct Cursor {
    line: usize,
    column: usize,
    shape: CursorShape,
}

impl std::fmt::Debug for Console {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Console")
            .field("columns", &self.columns)
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

/// The terminal's settings: no scrollback, no clipboard, and a blinking
/// block for a cursor, which blinks as the Linux console's does.
fn config() -> Config {
    Config {
        scrolling_history: 0,
        osc52: Osc52::Disabled,
        default_cursor_style: CursorStyle {
            shape: CursorShape::Block,
            blinking: true,
        },
        ..Config::default()
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
            grid,
            frame: Frame::new(pixels.0, pixels.1),
            columns,
            rows,
            replies: Vec::new(),
            blink: Blink::default(),
            drawn_rows: vec![None; usize::from(rows)],
        };
        console.draw();
        console
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
        self.answer();
    }

    /// When the console must be woken: a synchronized update ends, or the
    /// cursor blinks.
    pub fn deadline(&self) -> Option<Instant> {
        let sync = self.parser.sync_timeout().sync_timeout();
        match (sync, self.blink.at) {
            (Some(sync), Some(blink)) => Some(sync.min(blink)),
            (sync, blink) => sync.or(blink),
        }
    }

    /// Ends a synchronized update whose time ran out at `now`, and blinks
    /// the cursor when its time came.
    pub fn tick(&mut self, now: Instant) {
        let sync = self.parser.sync_timeout().sync_timeout();
        if sync.is_some_and(|deadline| deadline <= now) {
            self.parser.stop_sync(&mut self.term);
            self.answer();
        }
        self.time_blink(now);
    }

    /// Starts, stops or turns the cursor's blinking at `now`: it runs only
    /// while a blinking cursor is shown, and starts shown after output.
    fn time_blink(&mut self, now: Instant) {
        let shape = self.term.renderable_content().cursor.shape;
        if shape == CursorShape::Hidden || !self.term.cursor_style().blinking {
            self.blink = Blink::default();
            return;
        }
        let blink = &mut self.blink;
        if std::mem::take(&mut blink.fed) || blink.at.is_none() {
            blink.off = false;
            blink.at = Some(now + BLINK);
        } else if blink.at.is_some_and(|at| at <= now) {
            blink.off = !blink.off;
            blink.at = Some(now + BLINK);
        }
    }

    /// Turns the terminal's events into the answers the programs get.
    fn answer(&mut self) {
        let events: Vec<Event> = self.events.0.borrow_mut().drain(..).collect();
        for event in events {
            let text = match event {
                Event::PtyWrite(text) => text,
                Event::TextAreaSizeRequest(format) => format(self.window_size()),
                Event::ColorRequest(index, format) => {
                    let colour = colour::by_index(self.term.colors(), index);
                    format(TermRgb {
                        r: colour.0,
                        g: colour.1,
                        b: colour.2,
                    })
                }
                _ => continue,
            };
            self.replies.extend_from_slice(text.as_bytes());
        }
    }

    /// Takes the answers owed to the programs.
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// Whether the cursor keys send their application sequences (DECCKM).
    pub fn is_application_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    /// The frame drawn on.
    pub fn frame_mut(&mut self) -> &mut Frame {
        &mut self.frame
    }

    /// Draws what changed since the last call into the frame; returns how
    /// many cells were drawn, a scroll counting as one.
    pub fn draw(&mut self) -> usize {
        let damaged: Option<Vec<(usize, usize, usize)>> = match self.term.damage() {
            TermDamage::Full => None,
            TermDamage::Partial(lines) => Some(
                lines
                    .map(|bounds| (bounds.line, bounds.left, bounds.right))
                    .collect(),
            ),
        };
        self.term.reset_damage();
        let cursor = self.drawn_cursor();
        match damaged {
            None => self.draw_screen(cursor),
            Some(lines) => self.draw_lines(&lines, cursor),
        }
    }

    /// Where the cursor is drawn, and its shape: nowhere while it is
    /// hidden, or blinked off.
    fn drawn_cursor(&self) -> Option<Cursor> {
        let cursor = self.term.renderable_content().cursor;
        let line = usize::try_from(cursor.point.line.0).ok()?;
        (cursor.shape != CursorShape::Hidden && !self.blink.off).then_some(Cursor {
            line,
            column: cursor.point.column.0,
            shape: cursor.shape,
        })
    }

    /// Draws the cells of `lines` (line, first and last column), and keeps
    /// the hashes of their rows.
    fn draw_lines(&mut self, lines: &[(usize, usize, usize)], cursor: Option<Cursor>) -> usize {
        let mut drawn = 0;
        for &(line, left, right) in lines {
            for column in left..=right.min(usize::from(self.columns) - 1) {
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
        let rows = usize::from(self.rows);
        let next: Vec<u64> = (0..rows).map(|line| self.row_hash(line, cursor)).collect();
        let shift = scroll::best_shift(&self.drawn_rows, &next);
        let (_, cell) = self.grid.cell();
        let pixels = |rows: usize| u32::try_from(rows).unwrap_or(u32::MAX).saturating_mul(cell);
        match shift {
            Shift::None => {}
            Shift::Up(by) => self.frame.shift_rows(0, pixels(rows), pixels(by), true),
            Shift::Down(by) => self.frame.shift_rows(0, pixels(rows), pixels(by), false),
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
        drawn
    }

    /// A hash of the row at `line` as it is drawn, the cursor included.
    fn row_hash(&self, line: usize, cursor: Option<Cursor>) -> u64 {
        let mut hasher = DefaultHasher::new();
        for column in 0..usize::from(self.columns) {
            look(&self.term, line, column, cursor).hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Draws the cell at `line`, `column`, with the cursor over it when it
    /// is there.
    fn draw_cell(&mut self, line: usize, column: usize, cursor: Option<Cursor>) {
        let (Ok(x), Ok(y)) = (u16::try_from(column), u16::try_from(line)) else {
            return;
        };
        let Some((c, style, marks, shape)) = look(&self.term, line, column, cursor) else {
            return;
        };
        self.grid.draw(&mut self.frame, x, y, c, style);
        // Combining marks, such as the accent of an `e` followed by U+0301,
        // are drawn over the character they belong to.
        for &mark in marks {
            self.grid.draw_mark(&mut self.frame, x, y, mark, style.fg);
        }
        if let Some(shape) = shape {
            let area = self.grid.area(x, y);
            draw_cursor(&mut self.frame, area, shape, style.fg);
        }
    }

    /// Wipes what the terminal shows and the pixels drawn from it: the
    /// screen is cleared, its cells reset, the frame painted black.
    pub fn wipe(&mut self) {
        self.feed(b"\x1b[0m\x1b[H\x1b[2J\x1b[3J");
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

    /// The text of the screen, one line per row, without trailing spaces,
    /// combining marks after their characters.
    pub fn text(&self) -> String {
        let grid = self.term.grid();
        (0..grid.screen_lines())
            .map(|line| {
                let row = &grid[Line(i32::try_from(line).unwrap_or(i32::MAX))];
                let text: String = (0..grid.columns())
                    .map(|column| &row[Column(column)])
                    .filter(|cell| !cell.flags.contains(Flags::WIDE_CHAR_SPACER))
                    .flat_map(|cell| {
                        std::iter::once(cell.c)
                            .chain(cell.zerowidth().unwrap_or_default().iter().copied())
                    })
                    .collect();
                text.trim_end().to_owned() + "\n"
            })
            .collect()
    }
}

/// What the cell of `term` at `line`, `column` shows: its character, its
/// style, its combining marks, and the shape of `cursor` when it is over
/// it.
fn look(
    term: &Term<Events>,
    line: usize,
    column: usize,
    cursor: Option<Cursor>,
) -> Option<(char, CellStyle, &[char], Option<CursorShape>)> {
    let row = i32::try_from(line).ok()?;
    let cell = &term.grid()[Line(row)][Column(column)];
    let shape = cursor
        .filter(|cursor| cursor.line == line && cursor.column == column)
        .map(|cursor| cursor.shape);
    let colours = term.colors();
    let style = if shape == Some(CursorShape::Block) {
        colour::cursor(colours, cell)
    } else {
        colour::style(colours, cell)
    };
    let c = if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
        ' '
    } else {
        cell.c
    };
    Some((c, style, cell.zerowidth().unwrap_or_default(), shape))
}

/// Draws a cursor of `shape` other than the block, which swaps the cell's
/// colours, over `area`, in `colour`: an underline, a bar on the left, or
/// a hollow box, as thick as an eighth of the cell's width.
fn draw_cursor(frame: &mut Frame, area: Area, shape: CursorShape, colour: Rgb) {
    let thickness = (area.width / 8).max(1);
    let bottom = Area {
        y: area.y + area.height.saturating_sub(thickness),
        height: thickness.min(area.height),
        ..area
    };
    let left = Area {
        width: thickness.min(area.width),
        ..area
    };
    let parts = match shape {
        CursorShape::Underline => vec![bottom],
        CursorShape::Beam => vec![left],
        CursorShape::HollowBlock => vec![
            bottom,
            left,
            Area {
                height: bottom.height,
                ..area
            },
            Area {
                x: area.x + area.width.saturating_sub(thickness),
                ..left
            },
        ],
        CursorShape::Block | CursorShape::Hidden => Vec::new(),
    };
    for part in parts {
        frame.fill(part, colour);
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        self.wipe();
    }
}

#[cfg(test)]
mod tests;
