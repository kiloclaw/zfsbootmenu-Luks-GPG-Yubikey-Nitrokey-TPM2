use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use zbm_display::keys::Key;
use zbm_types::Untrusted;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::theme::Theme;

/// The cells of the screen `model` draws at `width` by `height`.
pub(crate) fn cells(model: &Model, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| view(model, frame)).unwrap();
    terminal.backend().buffer().clone()
}

/// The text of the screen `model` draws at `width` by `height`.
pub(crate) fn screen(model: &Model, width: u16, height: u16) -> String {
    let buffer = cells(model, width, height);
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

/// The demo in the classic menu (`zbm.menu=classic`), started: the tests
/// that want the workspace set its map.
fn started(capabilities: Capabilities) -> Session {
    let options = DemoOptions {
        classic: true,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(capabilities));
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
    session.key(Key::Escape);
    for key in ['s', 'k', 'p', '!', 'l', '?', ':'] {
        session.key(Key::Char(key));
        let text = screen(session.model(), 52, 20);
        assert!(text.lines().count() == 20, "{key}: {text}");
        session.key(Key::Escape);
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

/// The first cell of `text` on row `y`.
fn find(buffer: &Buffer, y: u16, text: &str) -> ratatui::buffer::Cell {
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
    let buffer = cells(session.model(), 80, 25);

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
    let mut model = started(Capabilities::FULL).into_model();
    model.snapshot.log.entries = (0..30)
        .map(|n| crate::menu::world::LogEntry {
            level: crate::menu::world::Level::Info,
            text: Untrusted::new(format!(
                "entry {n:02} {}",
                "long words that wrap ".repeat(5)
            )),
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
    let mut model = started(Capabilities::FULL).into_model();
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

#[test]
fn hostile_names_are_shown_escaped() {
    let mut model = started(Capabilities::FULL).into_model();
    let name = model.selected_environment().unwrap().name.clone();
    let environment = model
        .snapshot
        .environments
        .iter_mut()
        .find(|environment| environment.name == name)
        .unwrap();
    let kernel = Untrusted::new("6.1\u{1b}[2J".to_owned());
    environment.kernels = vec![crate::menu::world::Kernel {
        version: kernel.clone(),
        is_default: true,
    }];
    environment.command_line.value = Untrusted::new("quiet\u{202e}x\u{9b}1m".to_owned());
    let line = environment.command_line.value.get().clone();
    model.snapshot.volumes[0].name = Untrusted::new("luks\u{7}a".to_owned());
    model.snapshot.log.entries = vec![crate::menu::world::LogEntry {
        level: crate::menu::world::Level::Warning,
        text: Untrusted::new("hi\u{1b}]0;x\u{7}".to_owned()),
    }];
    model.focus = Some(name.clone());
    model.refresh();

    let main = screen(&model, 120, 40);
    model.screen = Screen::Kernels;
    let kernels = screen(&model, 80, 25);
    model.editor = Some(crate::menu::model::Editor {
        environment: name,
        kernel: Some(kernel),
        text: line,
    });
    model.screen = Screen::CommandLine;
    let editor = screen(&model, 80, 25);
    model.screen = Screen::Pools;
    let pools = screen(&model, 80, 25);
    model.screen = Screen::Log;
    let log = screen(&model, 80, 25);

    for (text, escaped) in [
        (&main, "6.1\\x1b[2J"),
        (&main, "quiet\\u{202e}x\\x9b1m"),
        (&kernels, "6.1\\x1b[2J"),
        // The title is drawn in capitals.
        (&editor, "6.1\\X1B[2J"),
        (&editor, "quiet\\u{202e}x\\x9b1m"),
        (&pools, "luks\\x07a"),
        (&log, "hi\\x1b]0;x\\x07"),
    ] {
        assert!(text.contains(escaped), "{escaped} not in\n{text}");
        let raw = |row: &str| row.contains(zbm_types::must_escape);
        assert!(!text.lines().any(raw), "{text:?}");
    }
}

#[test]
fn the_workspace_s_help_says_the_zfs_version_and_names_the_aliases_apart() {
    let mut model = started(Capabilities::FULL).into_model();
    model.screen = Screen::Help(crate::menu::keymap::Context::Main);
    let classic = screen(&model, 80, 90);
    model.map = crate::menu::keymap::Map::Workspace;
    let workspace = screen(&model, 80, 90);
    model.snapshot.zfs_version = Some(Untrusted::new("2.2\u{1b}[2J".to_owned()));
    let hostile = screen(&model, 80, 90);
    model.snapshot.zfs_version = None;
    let unknown = screen(&model, 80, 90);

    assert!(!classic.contains("ZFS 2.2.6-1"), "{classic}");
    assert!(classic.contains("CLASSIC BINDINGS"), "{classic}");
    assert!(workspace.contains("ZFS 2.2.6-1"), "{workspace}");
    assert!(workspace.contains("FZF ALIASES"), "{workspace}");
    assert!(!workspace.contains("CLASSIC"), "{workspace}");
    assert!(hostile.contains("ZFS 2.2\\x1b[2J"), "{hostile}");
    assert!(!unknown.contains("ZFS "), "{unknown}");
}

#[test]
fn the_zfs_version_stays_in_the_help_header_when_it_scrolls() {
    let mut model = started(Capabilities::FULL).into_model();
    model.screen = Screen::Help(crate::menu::keymap::Context::Main);
    model.map = crate::menu::keymap::Map::Workspace;
    model.scroll = 5;
    let scrolled = screen(&model, 52, 20);
    let header = |text: &str| {
        text.lines()
            .find(|line| line.contains("KEYS"))
            .unwrap_or_default()
            .to_owned()
    };
    model.snapshot.zfs_version = Some(Untrusted::new("2".repeat(80)));
    let long = screen(&model, 52, 20);

    assert!(header(&scrolled).contains("ZFS 2.2.6-1"), "{scrolled}");
    assert!(header(&long).contains("ZFS 222"), "{long}");
    assert!(header(&long).contains("KEYS, UP/DOWN TO SCROLL"), "{long}");
    assert!(header(&long).contains('…'), "{long}");
}

#[test]
fn the_line_view_s_help_says_the_zfs_version_in_the_workspace_only() {
    let mut model = started(Capabilities::FULL).into_model();
    model.screen = Screen::Help(crate::menu::keymap::Context::Main);
    let (classic, _) = super::words::rows(&model);
    model.map = crate::menu::keymap::Map::Workspace;
    let (workspace, _) = super::words::rows(&model);

    assert!(
        !classic.iter().any(|row| row.starts_with("ZFS ")),
        "{classic:#?}"
    );
    assert_eq!(workspace.first().map(String::as_str), Some("ZFS 2.2.6-1"));
}

#[test]
fn the_line_view_says_which_snapshots_are_marked() {
    for workspace in [true, false] {
        let options = DemoOptions {
            classic: !workspace,
            ..DemoOptions::default()
        };
        let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
        session.start();
        session.key(Key::Escape);
        session.key(Key::Char(if workspace { '2' } else { 's' }));
        let name = session
            .model()
            .selected_snapshot()
            .expect("a snapshot")
            .name
            .clone();
        session.key(Key::Char(' '));

        let (rows, _) = super::words::rows(session.model());

        let marked: Vec<&String> = rows
            .iter()
            .filter(|row| row.ends_with(", marked"))
            .collect();
        assert_eq!(marked.len(), 1, "{workspace}: {rows:#?}");
        assert!(marked[0].starts_with(name.name()), "{workspace}: {rows:#?}");
    }
}

#[test]
fn a_package_database_s_manager_and_versions_are_shown_escaped() {
    use crate::menu::world::{Event, PackageChange, PackageDiff, PackageKind};

    let model = started(Capabilities::FULL).into_model();
    let packages = PackageDiff {
        manager: "xbps\u{2066}".to_owned(),
        changes: vec![PackageChange {
            name: "mesa".to_owned(),
            kind: PackageKind::Upgraded,
            before: Some("24.1\u{202e}".to_owned()),
            after: Some("24.2\u{1b}[2J".to_owned()),
            files: Vec::new(),
        }],
    };
    let ready = Event::DiffReady {
        from: "zroot/ROOT/void@before".parse().unwrap(),
        to: None,
        changes: Vec::new(),
        packages: Some(packages),
    };
    let (model, _) = crate::menu::update(model, crate::menu::Message::Event(Box::new(ready)));
    let (model, _) = crate::menu::update(
        model,
        crate::menu::Message::Key(zbm_display::keys::KeyPress::new(Key::Right)),
    );

    let text = screen(&model, 120, 40);

    // The header is drawn in capitals.
    for escaped in ["XBPS\\U{2066}", "24.1\\u{202e} → 24.2\\x1b[2J"] {
        assert!(text.contains(escaped), "{escaped} not in\n{text}");
    }
    let buffer = cells(&model, 120, 40);
    let raw = |cell: &ratatui::buffer::Cell| cell.symbol().contains(zbm_types::must_escape);
    assert!(!buffer.content().iter().any(raw), "{text}");
}
