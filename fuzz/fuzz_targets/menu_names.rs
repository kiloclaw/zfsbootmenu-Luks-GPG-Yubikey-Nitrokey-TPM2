//! Hostile names through the menu's view (P13 of docs/dev/threat-model.md):
//! the text a pool, the files of its boot environments, crypttab and the
//! programs supply (A2, A3) reaches the screen. The input, split at its
//! newlines, gives the kernel versions, the command line, the LUKS volumes'
//! names and devices, the names and notes of the pools' devices, the name
//! of the host that used a pool not imported, the log, the version of the
//! ZFS module and the host ID; with the demo's snapshots read, every screen
//! is drawn with them, the workspace's Boot tab and help too, then the
//! workspace's Snapshots tab: its timeline of the snapshots whose names
//! the input gives, taken at times around now, after it and at the epoch,
//! with a group folded, and in the line view's words; at one of
//! three sizes that the first byte picks, the largest wide enough for the
//! host ID and the time. Drawing may not panic, and no cell may hold a
//! character that acts on a terminal.

#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use zbm_display::keys::{Key, KeyPress};
use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::demo::DemoOptions;
use zbm_tui::menu::demo::data::{machine, snapshots};
use zbm_tui::menu::keymap::{Context, Map};
use zbm_tui::menu::lines::page;
use zbm_tui::menu::model::{Editor, Model, Screen};
use zbm_tui::menu::theme::Theme;
use zbm_tui::menu::view::view;
use zbm_tui::menu::world::{
    DeviceState, Event, Kernel, Level, LogEntry, PoolHost, Snapshot, SnapshotInfo, SnapshotList,
};
use zbm_tui::menu::{Message, update};
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

/// The demo's machine, the snapshots of every environment read: made
/// once.
fn demo() -> &'static Snapshot {
    static DEMO: OnceLock<Snapshot> = OnceLock::new();
    DEMO.get_or_init(|| {
        let options = DemoOptions::default();
        let mut snapshot = machine(&options);
        snapshot.loaded = snapshots(&options);
        snapshot
    })
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
    for pool in &mut model.snapshot.pools {
        for device in &mut pool.devices {
            device.name = next();
            device.note = Some(next());
            device.state = Some(DeviceState::Unavailable);
            for child in &mut device.children {
                child.name = next();
                child.state = Some(DeviceState::Faulted);
            }
        }
    }
    if let Some(mut other) = model.snapshot.pools.first().cloned() {
        other.imported = false;
        other.id = Some(9);
        other.host = Some(PoolHost {
            hostid: None,
            hostname: Some(next()),
        });
        model.snapshot.pools.push(other);
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
    // The workspace's Boot tab, with what needs attention, its kernels
    // column under the path, with the inspector that wraps, and its help,
    // under the ZFS version; a host ID that is not one is none.
    model.snapshot.zfs_version = Some(next());
    model.snapshot.host = next().parse().ok();
    model.map = Map::Workspace;
    model.editor = None;
    model.refresh();
    for screen in [Screen::Main, Screen::Kernels, Screen::Help(Context::Main)] {
        model.screen = screen;
        check(&model, width, height);
    }
    // The workspace's Snapshots tab: hostile names, taken at times that
    // cross the boundaries of the groups, one group folded.
    let now = model.snapshot.now;
    let mut taken = Vec::new();
    for hours in 0..12_u64 {
        let part = next();
        let Ok(snapshot) = format!("{name}@{}", part.get()).parse() else {
            continue;
        };
        let length = u64::try_from(part.get().len()).unwrap_or(u64::MAX);
        let created = match length % 4 {
            0 => 0,
            1 => now.saturating_add(hours * 3600),
            2 => now.saturating_sub(length.saturating_mul(hours).saturating_mul(86_400)),
            _ => u64::MAX,
        };
        taken.push(SnapshotInfo {
            name: snapshot,
            created,
            used: length.saturating_mul(1 << 40),
        });
    }
    // As the machine sends them, so that the timeline groups them anew.
    let loaded = Event::SnapshotsLoaded(SnapshotList {
        environment: name.clone(),
        snapshots: taken,
    });
    model.focus = Some(name);
    model.screen = Screen::Snapshots;
    let (model, _) = update(model, Message::Event(Box::new(loaded)));
    check(&model, width, height);
    let _ = page(&model);
    let fold = Message::Key(KeyPress::new(Key::Char('z')));
    let (model, _) = update(model, fold);
    check(&model, width, height);
    let _ = page(&model);
});
