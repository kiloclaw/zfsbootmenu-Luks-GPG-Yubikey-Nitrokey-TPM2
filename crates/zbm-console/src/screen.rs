//! The terminal: Alacritty's state machine fed with what the programs
//! write, its cells drawn as pixels with the image's console font.
//!
//! Only what changed is drawn again (Alacritty's damage), and only the
//! area drawn reaches the display (`Frame::take_damage`), so that a
//! firmware framebuffer, which is slow to write, keeps up with a shell.
//!
//! There is no scrollback: what scrolls off the screen is gone, so a
//! passphrase shown by mistake does not stay in memory, and the answers the
//! terminal owes the programs (cursor position, device attributes, colours)
//! are kept apart for the caller to write back.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Osc52, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, Processor, Rgb as TermRgb};
use zbm_display::frame::Frame;
use zbm_display::grid::Grid;

use crate::colour;

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
    grid: Grid,
    frame: Frame,
    columns: u16,
    rows: u16,
    replies: Vec<u8>,
}

impl std::fmt::Debug for Console {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Console")
            .field("columns", &self.columns)
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

/// The terminal's settings: no scrollback, and no clipboard.
fn config() -> Config {
    Config {
        scrolling_history: 0,
        osc52: Osc52::Disabled,
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
        self.answer();
    }

    /// When a synchronized update must end, if one is under way.
    pub fn deadline(&self) -> Option<Instant> {
        self.parser.sync_timeout().sync_timeout()
    }

    /// Ends a synchronized update whose time ran out at `now`.
    pub fn tick(&mut self, now: Instant) {
        if self.deadline().is_some_and(|deadline| deadline <= now) {
            self.parser.stop_sync(&mut self.term);
            self.answer();
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
    /// many cells were drawn.
    pub fn draw(&mut self) -> usize {
        let damaged: Vec<(usize, usize, usize)> = match self.term.damage() {
            TermDamage::Full => (0..usize::from(self.rows))
                .map(|line| (line, 0, usize::from(self.columns) - 1))
                .collect(),
            TermDamage::Partial(lines) => lines
                .map(|bounds| (bounds.line, bounds.left, bounds.right))
                .collect(),
        };
        self.term.reset_damage();
        let cursor = self.term.renderable_content().cursor;
        let cursor = (cursor.shape != CursorShape::Hidden).then_some(cursor.point);
        let mut drawn = 0;
        for &(line, left, right) in &damaged {
            for column in left..=right.min(usize::from(self.columns) - 1) {
                let at_cursor = cursor.is_some_and(|point| {
                    point.line.0.try_into() == Ok(line) && point.column == Column(column)
                });
                self.draw_cell(line, column, at_cursor);
                drawn += 1;
            }
        }
        drawn
    }

    /// Draws the cell at `line`, `column`, with the block cursor over it
    /// when `at_cursor`.
    fn draw_cell(&mut self, line: usize, column: usize, at_cursor: bool) {
        let (Ok(row), Ok(x), Ok(y)) = (
            i32::try_from(line),
            u16::try_from(column),
            u16::try_from(line),
        ) else {
            return;
        };
        let cell = &self.term.grid()[Line(row)][Column(column)];
        let colours = self.term.colors();
        let style = if at_cursor {
            colour::cursor(colours, cell)
        } else {
            colour::style(colours, cell)
        };
        let c = if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            ' '
        } else {
            cell.c
        };
        self.grid.draw(&mut self.frame, x, y, c, style);
    }

    /// The text of the screen, one line per row, without trailing spaces.
    pub fn text(&self) -> String {
        let grid = self.term.grid();
        (0..grid.screen_lines())
            .map(|line| {
                let row = &grid[Line(i32::try_from(line).unwrap_or(i32::MAX))];
                let text: String = (0..grid.columns())
                    .map(|column| &row[Column(column)])
                    .filter(|cell| !cell.flags.contains(Flags::WIDE_CHAR_SPACER))
                    .map(|cell| cell.c)
                    .collect();
                text.trim_end().to_owned() + "\n"
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
