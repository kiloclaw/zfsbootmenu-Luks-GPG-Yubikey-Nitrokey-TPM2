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
    assert!(text.contains("* zroot/ROOT/void"), "{text}");
    assert!(text.contains("zroot/ROOT/ubuntu"), "{text}");
    assert!(text.contains("tank DEGRADED"), "{text}");
    assert!(text.contains("[Enter] boot"), "{text}");
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
        tall.contains("Newest snapshots") && tall.contains("* 6.12.9_1"),
        "{tall}"
    );
    assert!(!short.contains("Newest snapshots"), "{short}");
}
