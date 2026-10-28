//! The menu drawn as pixels: colours as on the console, the text where the
//! terminal draws it, and the loop.

use std::collections::VecDeque;

use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::style::Style;
use zbm_display::keys::{Key, KeyPress};
use zbm_display::psf::Font;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::DemoOptions;
use crate::menu::theme::Theme;
use crate::menu::view::tests::screen;

/// The image's Terminus 8×14, the font VGA text mode can draw too.
const TER_V14B: &[u8] = include_bytes!("../../../../../legacy/zfsbootmenu/fonts/ter-v14b.psf");

fn grid() -> Grid {
    Grid::new(Font::parse(TER_V14B).unwrap(), 1)
}

fn cell(fg: Color, bg: Color, modifier: Modifier) -> Cell {
    let mut cell = Cell::new("x");
    cell.set_style(Style::new().fg(fg).bg(bg).add_modifier(modifier));
    cell
}

#[test]
fn colours_are_the_console_s_and_the_256_colour_palette() {
    let default = Rgb(1, 2, 3);

    assert_eq!(rgb(Color::Gray, default), Rgb(0xaa, 0xaa, 0xaa));
    assert_eq!(rgb(Color::White, default), Rgb(0xff, 0xff, 0xff));
    assert_eq!(rgb(Color::DarkGray, default), Rgb(0x55, 0x55, 0x55));
    assert_eq!(rgb(Color::Indexed(3), default), Rgb(0xaa, 0x55, 0x00));
    assert_eq!(rgb(Color::Indexed(16), default), Rgb(0, 0, 0));
    assert_eq!(rgb(Color::Indexed(231), default), Rgb(255, 255, 255));
    assert_eq!(rgb(Color::Indexed(196), default), Rgb(255, 0, 0));
    assert_eq!(rgb(Color::Indexed(244), default), Rgb(128, 128, 128));
    assert_eq!(rgb(Color::Rgb(9, 8, 7), default), Rgb(9, 8, 7));
    assert_eq!(rgb(Color::Reset, default), default);
}

#[test]
fn bold_brightens_the_base_colours_and_the_other_modifiers_apply() {
    let bold = cell_style(&cell(Color::Green, Color::Reset, Modifier::BOLD));
    let bright = cell_style(&cell(Color::White, Color::Blue, Modifier::BOLD));
    let reversed = cell_style(&cell(Color::Gray, Color::Black, Modifier::REVERSED));
    let dim = cell_style(&cell(
        Color::White,
        Color::Reset,
        Modifier::DIM | Modifier::UNDERLINED,
    ));
    let plain = cell_style(&Cell::new("x"));
    let crossed = cell_style(&cell(Color::Gray, Color::Black, Modifier::CROSSED_OUT));

    assert_eq!((bold.fg, bold.bg), (Rgb(0x55, 0xff, 0x55), Rgb::BLACK));
    assert_eq!(
        (bright.fg, bright.bg),
        (Rgb(0xff, 0xff, 0xff), Rgb(0, 0, 0xaa))
    );
    assert_eq!(
        (reversed.fg, reversed.bg),
        (Rgb::BLACK, Rgb(0xaa, 0xaa, 0xaa))
    );
    assert_eq!(dim.fg, Rgb(0x7f, 0x7f, 0x7f));
    assert_eq!(dim.decoration, Decoration::underlined(true));
    assert_eq!(plain.decoration, Decoration::default());
    assert!(crossed.decoration.strikeout);
    assert_eq!((plain.fg, plain.bg), (Rgb(0xaa, 0xaa, 0xaa), Rgb::BLACK));
}

/// The demo's main view on the console theme.
fn model() -> crate::menu::Model {
    let mut session = Session::new(&DemoOptions::default(), Theme::new(Capabilities::CONSOLE));
    session.start();
    session.into_model()
}

#[test]
fn the_pixels_hold_the_text_the_terminal_would_show_in_its_colours() {
    let model = model();
    // 80×25 cells of 8×14 pixels, with a margin that fits no cell.
    let backend = PixelBackend::new(grid(), (80 * 8 + 5, 25 * 14 + 9));
    let mut pixels = Terminal::new(backend).unwrap();
    let mut cells = Terminal::new(TestBackend::new(80, 25)).unwrap();

    pixels.draw(|frame| view(&model, frame)).unwrap();
    cells.draw(|frame| view(&model, frame)).unwrap();

    let backend = pixels.backend_mut();
    assert_eq!(backend.size().unwrap(), Size::new(80, 25));
    assert_eq!(backend.text(), screen(&model, 80, 25));
    let window = backend.window_size().unwrap();
    assert_eq!(window.pixels, Size::new(80 * 8 + 5, 25 * 14 + 9));
    let buffer = cells.backend().buffer();
    let frame = backend.frame_mut();
    let mut colours = std::collections::BTreeSet::new();
    for row in 0..25_u16 {
        for column in 0..80_u16 {
            let style = cell_style(&buffer[(column, row)]);
            for y in u32::from(row) * 14..u32::from(row + 1) * 14 {
                for x in u32::from(column) * 8..u32::from(column + 1) * 8 {
                    let pixel = frame.pixel(x, y).unwrap();
                    assert!(pixel == style.fg || pixel == style.bg, "{column},{row}");
                    colours.insert(pixel);
                }
            }
        }
    }
    // The console theme's roles: text, titles, muted text, the selection,
    // the default's mark and the status bar.
    assert!(colours.len() >= 6, "{colours:?}");
}

#[test]
fn clearing_blanks_the_pixels_and_the_text() {
    let mut backend = PixelBackend::new(grid(), (16, 14));
    backend
        .draw([(0, 0, &cell(Color::White, Color::Red, Modifier::empty()))].into_iter())
        .unwrap();
    backend.frame_mut().take_damage();

    backend.clear_region(ClearType::All).unwrap();

    assert_eq!(backend.text(), "\n");
    assert_eq!(backend.frame_mut().pixel(0, 0), Some(Rgb::BLACK));
    assert!(backend.frame_mut().take_damage().is_some());
    backend.set_cursor_position(Position::new(1, 0)).unwrap();
    assert_eq!(backend.get_cursor_position().unwrap(), Position::new(1, 0));
    backend.hide_cursor().unwrap();
    backend.show_cursor().unwrap();
    backend.flush().unwrap();
}

/// A screen that keeps how often it was shown a frame.
struct FakeScreen {
    presented: usize,
}

impl Screen for FakeScreen {
    fn size(&self) -> (u32, u32) {
        (80 * 8, 25 * 14)
    }

    fn present(&mut self, frame: &mut Frame) -> io::Result<()> {
        frame.take_damage();
        self.presented += 1;
        Ok(())
    }
}

/// Keys from a script: a press, or `None` for time passing without one;
/// time passes once it ends.
struct FakeKeys(VecDeque<Option<KeyPress>>);

impl KeySource for FakeKeys {
    fn next(&mut self, _: Duration) -> io::Result<Option<KeyPress>> {
        Ok(self.0.pop_front().flatten())
    }
}

fn press(key: Key) -> KeyPress {
    KeyPress {
        key,
        shift: false,
        ctrl: false,
        alt: false,
        logo: false,
        keypad: false,
    }
}

#[test]
fn the_loop_draws_takes_keys_and_ends_when_the_menu_boots() {
    let session = Session::new(&DemoOptions::default(), Theme::new(Capabilities::CONSOLE));
    let mut screen = FakeScreen { presented: 0 };
    let mut script: VecDeque<Option<KeyPress>> = std::iter::repeat_n(None, 20).collect();
    script.extend([Key::Down, Key::Enter, Key::Escape].map(|key| Some(press(key))));
    let mut keys = FakeKeys(script);
    let mut texts = Vec::new();

    let why = run(session, &mut screen, &mut keys, grid(), &mut |text| {
        texts.push(text.to_owned());
    })
    .unwrap();

    assert!(why.starts_with("Booting"), "{why}");
    assert_eq!(screen.presented, texts.len());
    assert!(
        texts.iter().any(|text| text.contains("tank/ROOT")),
        "{texts:?}"
    );
    assert!(texts.last().unwrap().starts_with("Booting"));
    assert!(keys.0.is_empty());
}

#[test]
fn without_a_card_nothing_is_taken_over() {
    let session = Session::new(&DemoOptions::default(), Theme::new(Capabilities::CONSOLE));
    let mut asked = false;

    let error = run_on_display(
        session,
        Some(Path::new("/nonexistent/dri/card0")),
        &mut |_| {
            asked = true;
            Ok(grid())
        },
        &mut |_| {},
    )
    .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(
        error.to_string().contains("/nonexistent/dri/card0"),
        "{error}"
    );
    assert!(!asked);
}
