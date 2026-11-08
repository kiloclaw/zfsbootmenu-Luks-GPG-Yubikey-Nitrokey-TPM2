//! The menu's side of the client channel: the hello, the frames as runs of
//! cells ended by their cursor, the keys and sizes the console sends, the
//! screen lent and taken.

use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::widgets::Paragraph;
use zbm_display::keys::Key;
use zbm_platform::event::spawn_scoped_traced;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::keymap::layouts_tests::{LAYOUTS, SixLayouts, typed};
use crate::menu::keymap::{Context, Map};
use crate::menu::model::Screen;
use crate::menu::theme::Theme;

const CAPS: Caps = Caps {
    columns: 12,
    rows: 3,
    true_colour: true,
};

/// What the console end got, as messages, until nothing more came.
fn received(console: &Connection) -> Vec<FromClient> {
    let mut messages = Vec::new();
    while let Some(packet) = console.receive(Some(Duration::from_millis(50))).unwrap() {
        if packet.is_empty() {
            break;
        }
        messages.push(FromClient::decode(&packet).unwrap());
    }
    messages
}

/// The text of each cell the runs of `messages` put on row `row`.
fn row_cells(messages: &[FromClient], row: u16) -> Vec<String> {
    let mut cells = Vec::new();
    for message in messages {
        if let FromClient::Cells(runs) = message {
            for run in runs.iter().filter(|run| run.row == row) {
                cells.extend(run.cells.iter().cloned());
            }
        }
    }
    cells
}

/// The console's side of a hello: its own, of `version`, and its caps.
fn answer(console: &Connection, version: u16) {
    let packet = console
        .receive(Some(Duration::from_secs(5)))
        .unwrap()
        .unwrap();
    assert_eq!(
        FromClient::decode(&packet),
        Some(FromClient::Hello { version: VERSION })
    );
    // A message the menu does not wait for comes first.
    console.send(&FromConsole::Bell.encode()).unwrap();
    console
        .send(&FromConsole::Hello { version }.encode())
        .unwrap();
    console.send(&FromConsole::Caps(CAPS).encode()).unwrap();
}

#[test]
fn the_console_answers_the_hello_with_what_it_shows() {
    let (menu, console) = Connection::pair().unwrap();
    // What the console sent a client before this one is dropped.
    console.send(&FromConsole::Caps(CAPS).encode()).unwrap();
    console
        .send(&FromConsole::Hello { version: VERSION }.encode())
        .unwrap();

    let caps = std::thread::scope(|scope| {
        spawn_scoped_traced(scope, || answer(&console, VERSION));
        handshake(&menu, Duration::from_secs(5))
    });

    assert_eq!(caps.unwrap(), Some(CAPS));
}

#[test]
fn a_console_of_another_version_or_none_is_not_drawn_on() {
    let (menu, console) = Connection::pair().unwrap();
    let caps = std::thread::scope(|scope| {
        spawn_scoped_traced(scope, || answer(&console, VERSION + 1));
        handshake(&menu, Duration::from_secs(5))
    });
    assert_eq!(caps.unwrap(), None);

    // A console that says nothing, or is gone.
    assert_eq!(handshake(&menu, Duration::from_millis(10)).unwrap(), None);
    drop(console);
    assert_eq!(
        handshake(&menu, Duration::from_secs(5)).ok().flatten(),
        None
    );
    // Without a descriptor, or with one that is not a number.
    assert!(connect(None).is_none());
    assert!(connect(Some("tty")).is_none());
}

#[test]
fn a_frame_goes_as_runs_of_cells_and_its_cursor_ends_it() {
    let (menu, console) = Connection::pair().unwrap();
    let mut terminal = Terminal::new(CellSurface::new(Arc::new(menu), CAPS)).unwrap();

    terminal
        .draw(|frame| {
            frame.render_widget(Paragraph::new("日本 ok"), frame.area());
            frame.set_cursor_position((4, 1));
        })
        .unwrap();
    terminal.backend_mut().present().unwrap();

    let messages = received(&console);
    // What changed from the blank screen before it: the spaces did not.
    assert_eq!(row_cells(&messages, 0), ["日", "", "本", "", "o", "k"]);
    let FromClient::Cells(runs) = &messages[0] else {
        panic!("{messages:?}");
    };
    assert_eq!(
        runs.iter().map(|run| run.column).collect::<Vec<_>>(),
        [0, 5]
    );
    assert_eq!(messages.last(), Some(&FromClient::Cursor(Some((4, 1)))));

    // A frame that changed nothing is its cursor alone.
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new("日本 ok"), frame.area()))
        .unwrap();
    terminal.backend_mut().present().unwrap();
    assert_eq!(received(&console), [FromClient::Cursor(None)]);
}

#[test]
fn a_clear_blanks_every_cell_and_a_secret_is_said_once() {
    let (menu, console) = Connection::pair().unwrap();
    let mut surface = CellSurface::new(Arc::new(menu), CAPS);

    surface.draw([(0, 0, &Cell::new("x"))].into_iter()).unwrap();
    surface.clear_region(ClearType::All).unwrap();
    surface.set_secret(true);
    surface.set_secret(true);
    surface.present().unwrap();
    surface.set_secret(false);

    let messages = received(&console);
    assert_eq!(messages.first(), Some(&FromClient::Secret(true)));
    for row in 0..3 {
        assert_eq!(row_cells(&messages, row), vec![" "; 12], "row {row}");
    }
    assert_eq!(messages.last(), Some(&FromClient::Secret(false)));
    surface.set_size(Size::new(80, 25));
    assert_eq!(Backend::size(&surface).unwrap(), Size::new(80, 25));
    assert_eq!(
        surface.window_size().unwrap().columns_rows,
        Size::new(80, 25)
    );
    surface.set_cursor_position((1, 2)).unwrap();
    assert_eq!(surface.get_cursor_position().unwrap(), Position::new(1, 2));
    surface.flush().unwrap();
}

#[test]
fn the_keys_and_the_sizes_come_and_the_screen_is_lent_and_taken() {
    let (menu, console) = Connection::pair().unwrap();
    let (mut shown, terminal) = std::io::pipe().unwrap();
    let mut keys = CellKeys::new(Arc::new(menu), CAPS).with_terminal(Box::new(terminal));
    let press = KeyPress::new(Key::Char('ä')).with_alt();
    let wait = Duration::from_secs(5);

    assert_eq!(keys.key(Duration::from_millis(1)).unwrap(), None);
    let messages = [
        FromConsole::Key {
            press,
            layout: "de".to_owned(),
        },
        FromConsole::Resize {
            columns: 100,
            rows: 30,
        },
    ];
    for message in &messages {
        console.send(&message.encode()).unwrap();
    }
    console.send(&[99]).unwrap();
    assert_eq!(keys.key(wait).unwrap(), Some(press));
    assert_eq!(keys.size(), Some((12, 3)));
    assert_eq!(keys.key(wait).unwrap(), None);
    assert_eq!(keys.size(), Some((100, 30)));
    assert_eq!(keys.key(wait).unwrap(), None);

    keys.release().unwrap();
    keys.take().unwrap();
    assert_eq!(received(&console), [FromClient::Lend, FromClient::Take]);
    // The programs' terminal was cleared for them.
    let mut cleared = [0; 7];
    std::io::Read::read_exact(&mut shown, &mut cleared).unwrap();
    assert_eq!(&cleared, b"\x1b[H\x1b[2J");
    assert!(format!("{keys:?}").contains("size"));

    drop(console);
    let error = keys.key(wait).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
}

/// The keys the console sends with each layout in `layouts`, read: where
/// the input says each was typed.
fn typed_on(layouts: &[&str]) -> Vec<Option<TypedOn>> {
    let (menu, console) = Connection::pair().unwrap();
    let mut keys = CellKeys::new(Arc::new(menu), CAPS);
    assert_eq!(keys.typed_on(), None);
    let mut said = Vec::new();
    for layout in layouts {
        let key = FromConsole::Key {
            press: KeyPress::new(Key::Char('q')),
            layout: (*layout).to_owned(),
        };
        console.send(&key.encode()).unwrap();
        assert!(keys.key(Duration::from_secs(5)).unwrap().is_some());
        said.push(keys.typed_on());
    }
    said
}

#[test]
fn a_key_from_the_console_brings_its_layout() {
    let on = |name: &str| Some(TypedOn::Layout(name.to_owned()));

    assert_eq!(typed_on(&["de", "ru"]), [on("de"), on("ru")]);
}

#[test]
fn a_key_from_the_console_s_serial_terminal_is_a_character() {
    assert_eq!(
        typed_on(&["", "de"]),
        [
            Some(TypedOn::Characters),
            Some(TypedOn::Layout("de".to_owned()))
        ]
    );
}

/// The keys of the sample, as a US keyboard types them from the Boot tab:
/// the kernels, the Snapshots tab, help, and `Ctrl-C` back to the tab.
fn sample() -> [KeyPress; 4] {
    let c = |c| KeyPress::new(Key::Char(c));
    [c('l'), c('2'), c('?'), c('c').with_ctrl()]
}

#[test]
fn the_console_s_cells_carry_each_layout_to_the_intent() {
    let options = DemoOptions::default();
    let theme = || Theme::new(Capabilities::FULL);
    let mut us = Session::new(&options, theme());
    us.start();
    let shown: Vec<_> = sample()
        .into_iter()
        .map(|press| {
            us.press(press);
            us.model().screen
        })
        .collect();
    // Each key changes the screen, so each checks the key it was typed as.
    assert_eq!(
        shown,
        [
            Screen::Kernels,
            Screen::Snapshots,
            Screen::Help(Context::Snapshots),
            Screen::Snapshots,
        ]
    );
    for name in LAYOUTS {
        let (menu, console) = Connection::pair().unwrap();
        let mut keys = CellKeys::new(Arc::new(menu), CAPS);
        let mut session = Session::on(SixLayouts::new(&options), theme(), Map::Workspace);
        session.start();
        for (press, screen) in sample().into_iter().zip(&shown) {
            for press in typed(name, Map::Workspace, press) {
                let layout = name.to_owned();
                console
                    .send(&FromConsole::Key { press, layout }.encode())
                    .unwrap();
                let press = keys.key(Duration::from_secs(5)).unwrap().unwrap();
                session.press_typed_on(press, keys.typed_on());
            }
            assert_eq!(session.model().screen, *screen, "{name}");
        }
    }
}
