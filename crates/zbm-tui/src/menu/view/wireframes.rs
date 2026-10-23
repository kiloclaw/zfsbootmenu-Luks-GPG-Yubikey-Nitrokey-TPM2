//! The wireframes of `docs/dev/menu-design/wireframes/`: every view of the
//! prototype at 120×40, 80×25 and 52×20, and the main dialogs in ASCII, as
//! snapshot tests. Change a view, then review and accept the new
//! wireframes with `cargo insta review` or `cargo xtask bless`.

use ratatui::crossterm::event::KeyCode;

use super::tests::screen;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::theme::Theme;

/// The sizes of the wireframes.
const SIZES: [(u16, u16); 3] = [(120, 40), (80, 25), (52, 20)];

/// A key press of a scenario.
#[derive(Clone, Copy)]
enum Key {
    Char(char),
    Code(KeyCode),
    Text(&'static str),
}

/// A view and how to reach it from power-on.
struct Wireframe {
    name: &'static str,
    options: DemoOptions,
    ticks: usize,
    keys: &'static [Key],
}

fn options(locked: bool, foreign: bool) -> DemoOptions {
    DemoOptions {
        locked,
        foreign,
        ..DemoOptions::default()
    }
}

/// Every wireframe.
fn wireframes() -> Vec<Wireframe> {
    use Key::{Char, Code, Text};
    let plain = || options(false, false);
    let frame = |name, options, ticks, keys| Wireframe {
        name,
        options,
        ticks,
        keys,
    };
    vec![
        frame("startup", plain(), 5, &[]),
        frame("main", plain(), 50, &[]),
        frame("main-filter", plain(), 50, &[Char('/'), Text("ta")]),
        frame("kernels", plain(), 50, &[Char('k')]),
        frame("snapshots", plain(), 50, &[Char('s'), Char('o')]),
        frame(
            "snapshot-actions",
            plain(),
            50,
            &[Char('s'), Char('o'), Code(KeyCode::Enter)],
        ),
        frame(
            "rollback-confirm",
            plain(),
            50,
            &[Char('s'), Char('o'), Char('r'), Text("vo")],
        ),
        frame(
            "command-line",
            plain(),
            50,
            &[Char('e'), Text(" systemd.unit=rescue.target")],
        ),
        frame("pools", plain(), 50, &[Char('p')]),
        frame("unlock-pin", options(true, false), 50, &[Text("12")]),
        frame("import-problem", options(false, true), 50, &[]),
        frame(
            "import-failed",
            options(false, true),
            50,
            &[Code(KeyCode::Enter)],
        ),
        frame("palette", plain(), 50, &[Char(':'), Text("2026-09")]),
        frame("recovery", plain(), 50, &[Char('!')]),
        frame("log", plain(), 50, &[Char('l')]),
        frame("help", plain(), 50, &[Char('?')]),
        frame("power", plain(), 50, &[Char('x')]),
        frame(
            "notification",
            plain(),
            50,
            &[Code(KeyCode::Down), Char('d')],
        ),
    ]
}

/// The session after a wireframe's scenario.
fn play(wireframe: &Wireframe, capabilities: Capabilities) -> Session {
    let mut session = Session::new(&wireframe.options, Theme::new(capabilities));
    for _ in 0..wireframe.ticks {
        let model = session.model();
        if model.started || model.dialog.is_some() {
            break;
        }
        session.tick(0);
    }
    for key in wireframe.keys {
        match *key {
            Key::Char(c) => session.key(KeyCode::Char(c)),
            Key::Code(code) => session.key(code),
            Key::Text(text) => session.type_text(text),
        }
    }
    session
}

fn settings() -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("../../../../../docs/dev/menu-design/wireframes");
    settings.set_prepend_module_to_snapshot(false);
    settings.set_omit_expression(true);
    settings
}

#[test]
fn every_view_at_three_sizes() {
    settings().bind(|| {
        for wireframe in wireframes() {
            let session = play(&wireframe, Capabilities::FULL);
            for (width, height) in SIZES {
                let name = format!("{}-{width}x{height}", wireframe.name);
                insta::assert_snapshot!(name, screen(session.model(), width, height));
            }
        }
    });
}

#[test]
fn the_main_views_and_dialogs_in_ascii() {
    let names = [
        "main",
        "snapshots",
        "unlock-pin",
        "rollback-confirm",
        "import-problem",
    ];
    settings().bind(|| {
        for wireframe in wireframes().into_iter().filter(|w| names.contains(&w.name)) {
            let session = play(&wireframe, Capabilities::ASCII);
            let text = screen(session.model(), 80, 25);
            assert!(text.is_ascii(), "{}: {text}", wireframe.name);
            insta::assert_snapshot!(format!("{}-80x25-ascii", wireframe.name), text);
        }
    });
}
