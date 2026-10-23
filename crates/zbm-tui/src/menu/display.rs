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
use std::path::Path;
use std::time::Duration;

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect, Size};
use ratatui::style::{Color, Modifier};
use ratatui::{Terminal, TerminalOptions, Viewport};
use zbm_display::display::{Display, Screen, first_card};
use zbm_display::frame::{Area, Frame, Rgb};
use zbm_display::grid::{CellStyle, Grid};
use zbm_display::input::{KeySource, Keyboards};
use zbm_display::keys::{Key, KeyPress};
use zbm_display::vt::{Terminal as VirtualTerminal, active_terminal};

use super::demo::Session;
use super::view::view;

/// How often time passes for the menu, when no key comes.
const TICK: Duration = Duration::from_millis(200);

/// How long the last frame stays before the menu ends.
const LAST_FRAME: Duration = Duration::from_millis(1500);

/// The 16 colours of the Linux console, in the order of their numbers.
const PALETTE: [Rgb; 16] = [
    Rgb(0x00, 0x00, 0x00),
    Rgb(0xaa, 0x00, 0x00),
    Rgb(0x00, 0xaa, 0x00),
    Rgb(0xaa, 0x55, 0x00),
    Rgb(0x00, 0x00, 0xaa),
    Rgb(0xaa, 0x00, 0xaa),
    Rgb(0x00, 0xaa, 0xaa),
    Rgb(0xaa, 0xaa, 0xaa),
    Rgb(0x55, 0x55, 0x55),
    Rgb(0xff, 0x55, 0x55),
    Rgb(0x55, 0xff, 0x55),
    Rgb(0xff, 0xff, 0x55),
    Rgb(0x55, 0x55, 0xff),
    Rgb(0xff, 0x55, 0xff),
    Rgb(0x55, 0xff, 0xff),
    Rgb(0xff, 0xff, 0xff),
];

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
    if let Some(colour) = console_number(colour).and_then(|n| PALETTE.get(usize::from(n))) {
        return *colour;
    }
    match colour {
        Color::Rgb(red, green, blue) => Rgb(red, green, blue),
        Color::Indexed(number) => indexed(number),
        _ => default,
    }
}

/// Colours 16 to 255 of the 256-colour palette: a 6×6×6 cube, then greys.
fn indexed(number: u8) -> Rgb {
    if number >= 232 {
        let grey = 8 + (number - 232) * 10;
        return Rgb(grey, grey, grey);
    }
    let cube = number.saturating_sub(16);
    let level = |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
    Rgb(level(cube / 36), level(cube / 6 % 6), level(cube % 6))
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
    let mut fg = rgb(fg_colour, PALETTE[7]);
    let mut bg = rgb(cell.bg, PALETTE[0]);
    if modifiers.contains(Modifier::DIM) {
        fg = Rgb(fg.0 / 2, fg.1 / 2, fg.2 / 2);
    }
    if modifiers.contains(Modifier::REVERSED) {
        std::mem::swap(&mut fg, &mut bg);
    }
    CellStyle {
        fg,
        bg,
        underline: modifiers.contains(Modifier::UNDERLINED),
    }
}

/// The key event the menu takes for a key press.
pub fn key_event(press: KeyPress) -> KeyEvent {
    let code = match press.key {
        Key::Char(c) => KeyCode::Char(c),
        Key::Enter => KeyCode::Enter,
        Key::Escape => KeyCode::Esc,
        Key::Backspace => KeyCode::Backspace,
        Key::Tab => KeyCode::Tab,
        Key::BackTab => KeyCode::BackTab,
        Key::Up => KeyCode::Up,
        Key::Down => KeyCode::Down,
        Key::Left => KeyCode::Left,
        Key::Right => KeyCode::Right,
        Key::Home => KeyCode::Home,
        Key::End => KeyCode::End,
        Key::PageUp => KeyCode::PageUp,
        Key::PageDown => KeyCode::PageDown,
        Key::Insert => KeyCode::Insert,
        Key::Delete => KeyCode::Delete,
        Key::F(number) => KeyCode::F(number),
    };
    let mut modifiers = KeyModifiers::NONE;
    // A character already says whether Shift was held.
    if press.shift && !matches!(press.key, Key::Char(_) | Key::BackTab) {
        modifiers |= KeyModifiers::SHIFT;
    }
    if press.ctrl {
        modifiers |= KeyModifiers::CONTROL;
    }
    if press.alt {
        modifiers |= KeyModifiers::ALT;
    }
    KeyEvent::new(code, modifiers)
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

/// Runs `session` on `screen`, drawn with `grid`, with keys from `keys`;
/// returns why the menu ended. After each frame, `shown` gets the text of
/// the screen.
///
/// # Errors
///
/// When the screen cannot be drawn on or the keys read.
pub fn run(
    mut session: Session,
    screen: &mut dyn Screen,
    keys: &mut dyn KeySource,
    grid: Grid,
    shown: &mut dyn FnMut(&str),
) -> io::Result<String> {
    let backend = PixelBackend::new(grid, screen.size());
    let size = backend.size;
    let viewport = Viewport::Fixed(Rect::new(0, 0, size.width, size.height));
    let mut terminal = Terminal::with_options(backend, TerminalOptions { viewport })?;
    loop {
        terminal.draw(|frame| view(session.model(), frame))?;
        let backend = terminal.backend_mut();
        screen.present(backend.frame_mut())?;
        shown(&backend.text());
        if let Some(why) = session.leaving() {
            // The last frame stays a moment; keys pressed meanwhile are lost.
            while keys.next(LAST_FRAME)?.is_some() {}
            return Ok(why.to_owned());
        }
        match keys.next(TICK)? {
            Some(press) => {
                let event = key_event(press);
                session.press(event.code, event.modifiers);
            }
            None => session.tick(u64::try_from(TICK.as_millis()).unwrap_or(u64::MAX)),
        }
    }
}

/// Runs `session` on the display: the DRM card `card` (else the first),
/// the keyboards, and the virtual terminal shown now taken over until the
/// menu ends. `grid` gives the font for the display's size in pixels;
/// `shown` gets the text of each frame.
///
/// # Errors
///
/// When there is no card, output or keyboard, or one cannot be used.
pub fn run_on_display(
    session: Session,
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
    let why = run(session, &mut display, &mut keys, grid, shown);
    // The output goes back to the kernel console before the terminal does.
    drop(display);
    drop(terminal);
    why
}

#[cfg(test)]
mod tests;
