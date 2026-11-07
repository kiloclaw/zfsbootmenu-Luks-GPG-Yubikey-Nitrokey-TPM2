//! Hostile names through the menu's view (P13 of docs/dev/threat-model.md):
//! the text a pool, the files of its boot environments, crypttab and the
//! programs supply (A2, A3) reaches the screen. The input, split at its
//! newlines, gives the kernel versions, the command line, the LUKS volumes'
//! names and devices, the names and notes of the pools' devices, the name
//! of the host that used a pool not imported, the log, the version of the
//! ZFS module and the host ID; with the demo's snapshots read, every screen
//! is drawn with them, the workspace's Boot tab and help too, start-up as
//! its Boot tab, with a stage that failed for a reason the input gives,
//! and its Log tab while start-up runs; then the workspace's Snapshots
//! tab: its timeline of the snapshots whose names the input gives, taken
//! at times around now, after it and at the epoch, with a group folded,
//! and in the line view's words; then the
//! workspace's Pools tab: the tree of pools the scan sends with the names
//! of their vdevs and devices, their notes and what `zpool` says the input
//! gives, a pool of another host, and volumes of no pool, each row
//! selected, and the kernel's messages about a device; at one of three
//! sizes that the first byte picks, the largest wide enough for the host
//! ID and the time. Drawing may not panic, and no cell may hold a
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
    Action, Device, DeviceClass, DeviceErrors, DeviceLog, DeviceState, Event, Health, Kernel,
    Level, LogEntry, Pool, PoolHost, Snapshot, SnapshotInfo, SnapshotList, Stage, StageState,
};
use zbm_tui::menu::{Message, update};
use zbm_types::{PoolName, Untrusted, must_escape};

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
    // Start-up as the Boot tab, a stage failed for a reason the input
    // gives, and the Log tab while it runs.
    let kept = std::mem::take(&mut model.snapshot.startup);
    model.snapshot.startup = vec![
        Stage {
            name: next().into_inner(),
            state: StageState::Failed(next().into_inner()),
        },
        Stage {
            name: next().into_inner(),
            state: StageState::Running,
        },
    ];
    for screen in [Screen::Startup, Screen::Log] {
        model.screen = screen;
        check(&model, width, height);
    }
    model.snapshot.startup = kept;
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
    pools_tab(model, &mut next, (width, height));
});

/// A device of the name `name` and the note `note`, with `children`.
fn device(name: Untrusted<String>, note: Untrusted<String>, children: Vec<Device>) -> Device {
    let length = u64::try_from(name.get().len()).unwrap_or(u64::MAX);
    Device {
        class: DeviceClass::Data,
        state: (length % 3 > 0).then_some(DeviceState::Degraded),
        errors: Some(DeviceErrors {
            read: length,
            write: u64::MAX,
            cksum: length.saturating_mul(1 << 20),
        }),
        children,
        name,
        note: Some(note),
    }
}

/// The pools of the input, as a scan sends them: one that the input names,
/// when its name is one, with a vdev of two devices, a log device, a spare
/// in use, shown twice as `zpool` shows it, and what `zpool` says; and one
/// of another host, not imported. The volumes are of no pool.
fn hostile_pools(next: &mut impl FnMut() -> Untrusted<String>) -> Vec<Pool> {
    let Ok(name) = next().get().parse().or_else(|_| "tank".parse::<PoolName>()) else {
        return Vec::new();
    };
    let mut pool = Pool::imported(name, Health::Degraded);
    let disks = vec![
        device(next(), next(), Vec::new()),
        device(next(), next(), Vec::new()),
    ];
    let log = Device {
        class: DeviceClass::Log,
        ..device(next(), next(), Vec::new())
    };
    let spare = device(next(), next(), Vec::new());
    let mut spares = Device {
        class: DeviceClass::Spare,
        ..spare.clone()
    };
    spares.state = None;
    pool.devices = vec![device(next(), next(), disks), log, spares];
    pool.devices[0].children.push(spare);
    pool.status = Some(next());
    pool.action = Some(next());
    pool.errors = Some(u64::MAX);
    let mut other = pool.clone();
    other.imported = false;
    other.id = Some(u64::MAX);
    other.host = Some(PoolHost {
        hostid: None,
        hostname: Some(next()),
    });
    vec![pool, other]
}

/// The workspace's Pools tab of `model` after a scan found the pools of
/// the input: each row selected, drawn and said; then `Enter` on the first
/// device of the first vdev, and the kernel's messages the input gives
/// about it.
fn pools_tab(model: Model, next: &mut impl FnMut() -> Untrusted<String>, size: (u16, u16)) {
    let (width, height) = size;
    // The machine changed: its volumes hold devices of no pool.
    let mut changed = model.snapshot.clone();
    for volume in &mut changed.volumes {
        volume.pool = None;
    }
    let event = Event::Changed(Box::new(changed));
    let (model, _) = update(model, Message::Event(Box::new(event)));
    let scanned = Event::PoolsScanned(hostile_pools(next));
    let (model, _) = update(model, Message::Event(Box::new(scanned)));
    let key = |model, key| update(model, Message::Key(KeyPress::new(key))).0;
    let mut model = key(model, Key::Char('3'));
    let rows = model.pools.matches.len();
    for _ in 0..rows {
        check(&model, width, height);
        let _ = page(&model);
        model = key(model, Key::Down);
    }
    // The first pool, its vdev, then the first device of it.
    let model = [Key::Home, Key::Down, Key::Down]
        .into_iter()
        .fold(model, key);
    let (model, actions) = update(model, Message::Key(KeyPress::new(Key::Enter)));
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::LoadDeviceLog(_))),
        "{actions:?}"
    );
    let log = DeviceLog {
        device: next(),
        names: vec![next(), next()],
        lines: (0..4).map(|_| next()).collect(),
    };
    let loaded = Event::DeviceLogLoaded(log);
    let (model, _) = update(model, Message::Event(Box::new(loaded)));
    check(&model, width, height);
    let _ = page(&model);
}
