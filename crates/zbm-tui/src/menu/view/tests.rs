use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::KeyCode;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::theme::Theme;

/// The text of the screen `model` draws at `width` by `height`.
pub(crate) fn screen(model: &Model, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| view(model, frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..height {
        let line: String = (0..width)
            .map(|x| buffer[(x, y)].symbol().to_owned())
            .collect();
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

fn started(capabilities: Capabilities) -> Session {
    let mut session = Session::new(&DemoOptions::default(), Theme::new(capabilities));
    session.start();
    session
}

#[test]
fn the_main_view_shows_the_countdown_the_list_and_the_details() {
    let session = started(Capabilities::FULL);

    let text = screen(session.model(), 80, 25);

    assert!(text.contains("Booting zroot/ROOT/void in 10 s"), "{text}");
    assert!(text.contains("▶ zroot/ROOT/void"), "{text}");
    assert!(text.contains("default  6.12.9_1"), "{text}");
    assert!(text.contains("zroot/ROOT/ubuntu"), "{text}");
    assert!(text.contains("tank DEGRADED"), "{text}");
    assert!(text.contains("Enter Boot"), "{text}");
    assert!(text.contains("? More"), "{text}");
}

#[test]
fn colour_and_monochrome_say_the_same() {
    let colour = started(Capabilities::FULL);
    let mono = started(Capabilities::MONOCHROME);

    assert_eq!(screen(colour.model(), 80, 25), screen(mono.model(), 80, 25));
}

#[test]
fn ascii_terminals_get_no_line_characters() {
    let session = started(Capabilities::ASCII);

    let text = screen(session.model(), 80, 25);

    assert!(text.is_ascii(), "{text}");
}

#[test]
fn a_too_small_terminal_gets_a_notice() {
    let session = started(Capabilities::FULL);

    let text = screen(session.model(), 40, 10);

    assert!(text.starts_with("The menu needs 52x20 at least."));
}

#[test]
fn every_view_draws_at_the_smallest_size() {
    let mut session = started(Capabilities::FULL);
    session.key(KeyCode::Esc);
    for key in ['s', 'k', 'p', '!', 'l', '?', ':'] {
        session.key(KeyCode::Char(key));
        let text = screen(session.model(), 52, 20);
        assert!(text.lines().count() == 20, "{key}: {text}");
        session.key(KeyCode::Esc);
    }
}

#[test]
fn the_unlock_dialog_does_not_show_the_length_of_the_secret() {
    let options = DemoOptions {
        locked: true,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();

    session.type_text("12");

    let text = screen(session.model(), 80, 25);
    assert!(text.contains("TPM2 PIN:"), "{text}");
    assert!(!text.contains('*'), "{text}");
}

#[test]
fn a_tall_detail_pane_lists_kernels_and_snapshots() {
    let session = started(Capabilities::FULL);

    let tall = screen(session.model(), 120, 40);
    let short = screen(session.model(), 80, 25);

    assert!(
        tall.contains("NEWEST SNAPSHOTS") && tall.contains("boots"),
        "{tall}"
    );
    assert!(!short.contains("NEWEST SNAPSHOTS"), "{short}");
}

/// The cells of the screen `model` draws at 80×25.
fn cells(model: &Model) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(80, 25)).unwrap();
    terminal.draw(|frame| view(model, frame)).unwrap();
    terminal.backend().buffer().clone()
}

/// The first cell of `text` on row `y`.
fn find(buffer: &ratatui::buffer::Buffer, y: u16, text: &str) -> ratatui::buffer::Cell {
    let row: String = (0..80)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect();
    let column = row
        .find(text)
        .unwrap_or_else(|| panic!("{text} not in {row}"));
    let x = u16::try_from(row[..column].chars().count()).unwrap();
    buffer[(x, y)].clone()
}

#[test]
fn on_the_console_the_banner_leads_with_what_boots_and_when() {
    use ratatui::style::{Color, Modifier};
    let session = started(Capabilities::CONSOLE);
    let buffer = cells(session.model());

    assert_eq!(find(&buffer, 1, "Booting").fg, Color::White);
    assert_eq!(find(&buffer, 1, "zroot/ROOT/void").fg, Color::White);
    assert_eq!(find(&buffer, 1, "10 s").fg, Color::Cyan);
    assert_eq!(find(&buffer, 1, "Press any key").fg, Color::Gray);
    // The selected row's own label takes the selection's colours, not bold.
    let label = find(&buffer, 3, "default");
    assert_eq!((label.fg, label.bg), (Color::Black, Color::Cyan));
    assert!(!label.modifier.contains(Modifier::BOLD));
    assert_eq!(find(&buffer, 24, "Kernels").fg, Color::Gray);
    assert_eq!(find(&buffer, 24, "k Kernels").fg, Color::Cyan);
}

#[test]
fn the_log_scrolls_to_its_last_entry_when_lines_wrap() {
    let mut model = started(Capabilities::FULL).model().clone();
    model.snapshot.log.entries = (0..30)
        .map(|n| crate::menu::world::LogEntry {
            level: crate::menu::world::Level::Info,
            text: format!("entry {n:02} {}", "long words that wrap ".repeat(5)),
        })
        .collect();
    model.screen = Screen::Log;
    model.scroll = 29;

    let text = screen(&model, 80, 25);

    assert!(text.contains("entry 29"), "{text}");
    assert!(!text.contains("entry 28"), "{text}");
}

#[test]
fn the_command_line_wraps_where_the_cursor_goes() {
    let mut model = started(Capabilities::FULL).model().clone();
    let environment = model.snapshot.environments[0].name.clone();
    // 78 characters on 76 columns: a word wrap would move all of `quietly`.
    let text = format!("{} quietly", "x".repeat(70));
    model.editor = Some(crate::menu::model::Editor {
        text: text.clone(),
        environment,
        kernel: None,
    });
    model.screen = Screen::CommandLine;

    let mut terminal = Terminal::new(TestBackend::new(80, 25)).unwrap();
    terminal.draw(|frame| view(&model, frame)).unwrap();
    let cursor = terminal.get_cursor_position().unwrap();
    let buffer = terminal.backend().buffer();

    let shown = screen(&model, 80, 25);
    // The character before the cursor is the last one typed.
    let before = &buffer[(cursor.x - 1, cursor.y)];
    assert_eq!(before.symbol(), "y", "cursor at {cursor:?}\n{shown}");
    assert_eq!(screens::wrapped("", 5).len(), 1);
    assert_eq!(screens::wrapped("abcdefg", 3).len(), 3);
}
