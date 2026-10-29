//! Running the menu on the display, without a terminal (ROADMAP.md #19,
//! step 1): Ratatui's cells drawn as pixels with the image's console font
//! (`zbm_display::grid`), presented on a DRM output, and keys read from the
//! keyboards' input devices.
//!
//! The pixels follow the Linux console: its 16-colour palette, bold drawn
//! bright for the eight base colours, and the [console
//! theme](crate::menu::caps::Capabilities::CONSOLE), so that the menu looks
//! the same on either.

use std::io;
use std::os::fd::BorrowedFd;
use std::path::Path;
use std::time::Duration;

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use ratatui::style::{Color, Modifier};
use zbm_display::decor::Decoration;
use zbm_display::display::{Display, Screen, first_card};
use zbm_display::faces::FontFace;
use zbm_display::frame::{Area, Frame, Rgb};
use zbm_display::grid::{CellStyle, Grid};
use zbm_display::input::{KeySource, Keyboards};
use zbm_display::keys::KeyPress;
use zbm_display::palette;
use zbm_display::vt::{Terminal as VirtualTerminal, active_terminal};
use zbm_platform::Clock;
use zbm_platform::clock::SystemClock;

use super::frontend::{Frontend, Input, Surface};
use super::machine::Machine;
use super::session::Session;

/// The console's number of a named colour; `None` for the others.
fn console_number(colour: Color) -> Option<u8> {
    Some(match colour {
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Indexed(number) if number < 16 => number,
        _ => return None,
    })
}

/// The pixels of a colour; `Reset` is `default`.
pub fn rgb(colour: Color, default: Rgb) -> Rgb {
    if let Some(number) = console_number(colour) {
        return palette::indexed(number);
    }
    match colour {
        Color::Rgb(red, green, blue) => Rgb(red, green, blue),
        Color::Indexed(number) => palette::indexed(number),
        _ => default,
    }
}

/// How a cell is drawn: its colours, bold drawing the eight base colours
/// bright as the console does, reversed swapping them, dim halving the
/// foreground.
pub fn cell_style(cell: &Cell) -> CellStyle {
    let modifiers = cell.modifier;
    let mut fg_colour = cell.fg;
    if modifiers.contains(Modifier::BOLD)
        && let Some(number @ 0..=7) = console_number(fg_colour)
    {
        fg_colour = Color::Indexed(number + 8);
    }
    let mut fg = rgb(fg_colour, palette::FOREGROUND);
    let mut bg = rgb(cell.bg, palette::BACKGROUND);
    if modifiers.contains(Modifier::DIM) {
        fg = palette::dim(fg);
    }
    if modifiers.contains(Modifier::REVERSED) {
        std::mem::swap(&mut fg, &mut bg);
    }
    CellStyle {
        fg,
        bg,
        decoration: Decoration {
            strikeout: modifiers.contains(Modifier::CROSSED_OUT),
            ..Decoration::underlined(modifiers.contains(Modifier::UNDERLINED))
        },
        face: FontFace::of(
            modifiers.contains(Modifier::BOLD),
            modifiers.contains(Modifier::ITALIC),
        ),
    }
}

/// A Ratatui backend that draws cells as pixels into a frame, and keeps
/// the text of the screen, which the VM tests read.
#[derive(Debug)]
pub struct PixelBackend {
    grid: Grid,
    frame: Frame,
    size: Size,
    text: Vec<String>,
    cursor: Position,
}

impl PixelBackend {
    /// Draws with `grid` on a black frame of `pixels` (width, height), in
    /// as many cells as fit.
    pub fn new(grid: Grid, pixels: (u32, u32)) -> PixelBackend {
        let (columns, rows) = grid.dimensions(pixels);
        PixelBackend {
            grid,
            frame: Frame::new(pixels.0, pixels.1),
            size: Size::new(columns, rows),
            text: vec![" ".to_owned(); usize::from(columns) * usize::from(rows)],
            cursor: Position::ORIGIN,
        }
    }

    /// The frame drawn on.
    pub fn frame_mut(&mut self) -> &mut Frame {
        &mut self.frame
    }

    /// The text of the screen, one line per row, without trailing spaces.
    pub fn text(&self) -> String {
        let columns = usize::from(self.size.width).max(1);
        self.text
            .chunks(columns)
            .map(|row| row.concat().trim_end().to_owned() + "\n")
            .collect()
    }
}

impl Backend for PixelBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            let symbol = cell.symbol();
            let c = symbol.chars().next().unwrap_or(' ');
            self.grid.draw(&mut self.frame, x, y, c, cell_style(cell));
            let index = usize::from(y) * usize::from(self.size.width) + usize::from(x);
            if let Some(slot) = self.text.get_mut(index) {
                symbol.clone_into(slot);
            }
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.cursor = position.into();
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        let (width, height) = self.frame.size();
        let all = Area {
            x: 0,
            y: 0,
            width,
            height,
        };
        self.frame.fill(all, Rgb::BLACK);
        self.text.iter_mut().for_each(|cell| " ".clone_into(cell));
        Ok(())
    }

    fn clear_region(&mut self, _: ClearType) -> io::Result<()> {
        self.clear()
    }

    fn size(&self) -> io::Result<Size> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        let (width, height) = self.frame.size();
        let pixels = Size::new(
            u16::try_from(width).unwrap_or(u16::MAX),
            u16::try_from(height).unwrap_or(u16::MAX),
        );
        Ok(WindowSize {
            columns_rows: self.size,
            pixels,
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The pixels of a [`PixelBackend`] presented on a screen after each frame,
/// whose text goes to `shown`: the surface of the menu on the display.
pub struct Presented<'s> {
    backend: PixelBackend,
    screen: &'s mut dyn Screen,
    shown: &'s mut dyn FnMut(&str),
}

impl<'s> Presented<'s> {
    /// `grid`'s cells on `screen`, in as many as fit; `shown` gets the text
    /// of each frame.
    pub fn new(
        grid: Grid,
        screen: &'s mut dyn Screen,
        shown: &'s mut dyn FnMut(&str),
    ) -> Presented<'s> {
        let backend = PixelBackend::new(grid, screen.size());
        Presented {
            backend,
            screen,
            shown,
        }
    }
}

impl Backend for Presented<'_> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.backend.draw(content)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.backend.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.backend.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.backend.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.backend.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.backend.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.backend.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        self.backend.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.backend.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.backend.flush()
    }
}

impl Surface for Presented<'_> {
    /// The display's size is its mode's: nothing tells it another.
    fn set_size(&mut self, _: Size) {}

    fn present(&mut self) -> io::Result<()> {
        self.screen.present(self.backend.frame_mut())?;
        (self.shown)(&self.backend.text());
        Ok(())
    }
}

/// The keys of the keyboards, as the front end's input. They do not wake
/// for the machine: what it sends shows within a tick.
pub struct Keyboard<'k>(pub &'k mut dyn KeySource);

impl Input for Keyboard<'_> {
    fn key_or_wake(
        &mut self,
        timeout: Duration,
        _: Option<BorrowedFd<'_>>,
    ) -> io::Result<Option<KeyPress>> {
        self.0.next(timeout)
    }
}

/// Runs `session` on `screen`, drawn with `grid`, with keys from `keys`,
/// its time on `clock`; returns why the menu ended. After each frame,
/// `shown` gets the text of the screen ([`Frontend::run`]).
///
/// # Errors
///
/// When the screen cannot be drawn on or the keys read.
pub fn run<M: Machine>(
    session: Session<M>,
    (screen, keys): (&mut dyn Screen, &mut dyn KeySource),
    grid: Grid,
    shown: &mut dyn FnMut(&str),
    clock: impl Clock + 'static,
) -> io::Result<String> {
    let surface = Presented::new(grid, screen, shown);
    let mut session = session;
    Frontend::new(surface, Keyboard(keys))?
        .with_clock(clock)
        .run(&mut session)
}

/// Runs `session` on the display: the DRM card `card` (else the first),
/// the keyboards, and the virtual terminal shown now taken over until the
/// menu ends. `grid` gives the font for the display's size in pixels;
/// `shown` gets the text of each frame.
///
/// # Errors
///
/// When there is no card, output or keyboard, or one cannot be used.
pub fn run_on_display<M: Machine>(
    session: Session<M>,
    card: Option<&Path>,
    grid: &mut dyn FnMut((u32, u32)) -> io::Result<Grid>,
    shown: &mut dyn FnMut(&str),
) -> io::Result<String> {
    let (sysfs, dev) = (Path::new("/sys"), Path::new("/dev"));
    let card = match card {
        Some(card) => card.to_path_buf(),
        None => {
            first_card(dev).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no DRM card"))?
        }
    };
    let mut display = Display::open(&card)?;
    let grid = grid(display.size())?;
    // Without a virtual terminal (a serial console only), nothing is taken.
    let terminal = active_terminal(sysfs, dev)
        .map(|path| VirtualTerminal::take(&path))
        .transpose()?;
    let mut keys = Keyboards::open(sysfs, dev)?;
    let why = run(
        session,
        (&mut display, &mut keys),
        grid,
        shown,
        SystemClock::new(),
    );
    // The output goes back to the kernel console before the terminal does.
    drop(display);
    drop(terminal);
    why
}

#[cfg(test)]
mod tests;
