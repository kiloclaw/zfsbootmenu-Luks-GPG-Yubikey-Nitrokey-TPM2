//! Hostile names through the menu's view (P13 of docs/dev/threat-model.md):
//! the text a pool, the files of its boot environments, crypttab and the
//! programs supply (A2, A3) reaches the screen. The input, split at its
//! newlines, gives the kernel versions, the command line, the LUKS volumes'
//! names and devices, and the log; every screen is drawn with them, at one
//! of three sizes that the first byte picks. Drawing may not panic, and no
//! cell may hold a character that acts on a terminal.

#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::demo::DemoOptions;
use zbm_tui::menu::demo::data::machine;
use zbm_tui::menu::model::{Editor, Model, Screen};
use zbm_tui::menu::theme::Theme;
use zbm_tui::menu::view::view;
use zbm_tui::menu::world::{Kernel, Level, LogEntry, Snapshot};
use zbm_types::{Untrusted, must_escape};

/// The sizes drawn: the smallest the menu lays out for, the console's, a
/// large one.
const SIZES: [(u16, u16); 3] = [(52, 20), (80, 25), (132, 43)];

/// The screens drawn.
const SCREENS: [Screen; 7] = [
    Screen::Main,
    Screen::Snapshots,
    Screen::Kernels,
    Screen::CommandLine,
    Screen::Pools,
    Screen::Recovery,
    Screen::Log,
];

/// The demo's machine: made once.
fn demo() -> &'static Snapshot {
    static DEMO: OnceLock<Snapshot> = OnceLock::new();
    DEMO.get_or_init(|| machine(&DemoOptions::default()))
}

/// Draws `model` at `width` by `height`, and checks every cell.
fn check(model: &Model, width: u16, height: u16) {
    let Ok(mut terminal) = Terminal::new(TestBackend::new(width, height)) else {
        return;
    };
    if terminal.draw(|frame| view(model, frame)).is_err() {
        return;
    }
    for cell in terminal.backend().buffer().content() {
        let symbol = cell.symbol();
        assert!(!symbol.contains(must_escape), "{symbol:?} drawn");
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&first, data)) = data.split_first() else {
        return;
    };
    let (width, height) = SIZES[usize::from(first) % SIZES.len()];
    let text = String::from_utf8_lossy(data);
    let mut parts = text.split('\n').map(|part| Untrusted::new(part.to_owned()));
    let mut next = || parts.next().unwrap_or_default();

    let mut model = Model::new(demo().clone(), Theme::new(Capabilities::FULL));
    let Some(name) = model
        .selected_environment()
        .map(|environment| environment.name.clone())
    else {
        return;
    };
    for environment in &mut model.snapshot.environments {
        environment.kernels = vec![
            Kernel {
                version: next(),
                is_default: true,
            },
            Kernel {
                version: next(),
                is_default: false,
            },
        ];
        environment.command_line.value = next();
    }
    for volume in &mut model.snapshot.volumes {
        volume.name = next();
        volume.device = next();
    }
    model.snapshot.log.entries = (0..3)
        .map(|_| LogEntry {
            level: Level::Warning,
            text: next(),
        })
        .collect();
    model.focus = Some(name.clone());
    model.refresh();

    for screen in SCREENS {
        model.screen = screen;
        model.editor = (screen == Screen::CommandLine).then(|| Editor {
            environment: name.clone(),
            kernel: Some(next()),
            text: next().into_inner(),
        });
        check(&model, width, height);
    }
});
