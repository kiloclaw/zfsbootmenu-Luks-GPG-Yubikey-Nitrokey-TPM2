//! Hostile names through the menu's view (P13 of docs/dev/threat-model.md):
//! the text a pool, the files of its boot environments, crypttab and the
//! programs supply (A2, A3) reaches the screen. The input, split at its
//! newlines, gives the kernel versions, the command line, the LUKS volumes'
//! names and devices, the names and notes of the pools' devices, the name
//! of the host that used a pool not imported, the log, the version of the
//! ZFS module and the host ID; with the demo's snapshots read, every screen
//! is drawn with them in each key map, the classic one and the
//! workspace's; the workspace's Boot tab and help too, start-up as its
//! Boot tab, with a stage that failed for a reason the input gives, and
//! its Log tab while start-up runs; then the workspace's Snapshots
//! tab: its timeline of the snapshots whose names the input gives, taken
//! at times around now, after it and at the epoch, with a group folded,
//! and in the line view's words; then the review pages of a rollback to a
//! snapshot the input names and of the rewind and the forced import of a
//! pool it names, while they are planned, then with plans whose lost
//! bookmark, checkpoint time and filesystem, steps' commands, clone, hold
//! tags, host and failure the input gives, refused and not; then the
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
use zbm_tui::menu::model::{Editor, Model, Screen, Then};
use zbm_tui::menu::theme::Theme;
use zbm_tui::menu::update::follow;
use zbm_tui::menu::view::view;
use zbm_tui::menu::world::{
    Access, Action, Device, DeviceClass, DeviceErrors, DeviceLog, DeviceState, Event, Health,
    ImportOptions, Keep, Kernel, Level, LogEntry, Loss, Outcome, Plan, Pool, PoolHost, PoolUse,
    Review, Safer, Snapshot, SnapshotInfo, SnapshotList, Stage, StageState, Step, StepKind, Stop,
};
use zbm_tui::menu::{Message, update};
use zbm_types::{DatasetName, ImportPolicy, PoolName, SnapshotName, Untrusted, must_escape};

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

    // Every screen in each key map: the classic menu's and the workspace's.
    for map in [Map::Classic, Map::Workspace] {
        model.map = map;
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
    let model = review_pages(model, &mut next, (width, height));
    pools_tab(model, &mut next, (width, height));
});

/// What a rollback to `target` loses, with the names and sizes the input
/// gives: every kind of loss, a filesystem the input names among them.
fn hostile_losses(
    next: &mut impl FnMut() -> Untrusted<String>,
    target: &SnapshotName,
    length: u64,
) -> Vec<Loss> {
    let made = format!("{}/{}", target.pool(), next().get());
    vec![
        Loss::Snapshot {
            name: target.clone(),
            used: Some(u64::MAX),
        },
        Loss::Bookmark {
            dataset: target.dataset().clone(),
            name: next().into_inner(),
        },
        Loss::Written {
            since: target.clone(),
            bytes: Some(length),
        },
        Loss::Newer(target.clone()),
        Loss::SinceCheckpoint {
            created: Some(next()),
            space: Some(length),
            discarding: true,
            datasets: Some(made.parse::<DatasetName>().into_iter().collect()),
            snapshots: None,
        },
    ]
}

/// What stops a change of `target`: the clone, the holds, one without its
/// tag, and the host the input names, then more holds than a page has
/// rows.
fn hostile_stops(
    next: &mut impl FnMut() -> Untrusted<String>,
    target: &SnapshotName,
    host: &PoolHost,
) -> Vec<Stop> {
    let mut stops = vec![
        Stop::Clone {
            snapshot: target.clone(),
            clone: next(),
        },
        Stop::Hold {
            snapshot: target.clone(),
            tag: Some(next()),
        },
        Stop::Hold {
            snapshot: target.clone(),
            tag: None,
        },
        Stop::Active {
            pool: target.pool(),
            host: Some(host.clone()),
        },
    ];
    let tag = next();
    stops.extend((0..40).map(|_| Stop::Hold {
        snapshot: target.clone(),
        tag: Some(tag.clone()),
    }));
    stops
}

/// The steps of `action` on `target`, their commands the input gives.
fn hostile_steps(
    next: &mut impl FnMut() -> Untrusted<String>,
    action: &Action,
    target: &SnapshotName,
) -> Vec<Step> {
    let mut steps = vec![Step {
        kind: StepKind::Rollback(target.clone()),
        command: Some(next().into_inner()),
    }];
    if let Action::Import(pool, options) = action {
        steps.push(Step {
            kind: StepKind::Import {
                pool: pool.clone(),
                options: options.clone(),
            },
            command: Some(next().into_inner()),
        });
    }
    steps.push(Step {
        kind: StepKind::Export(target.pool()),
        command: None,
    });
    steps
}

/// The safer action of `action`: a clone of `target` for a rollback, the
/// read-only import for a rewind, none for a forced import.
fn safer(action: &Action, target: &SnapshotName) -> Option<Safer> {
    match action {
        Action::Import(pool, options) if options.rewind => {
            let read_only = ImportOptions {
                access: Access::ReadOnly,
                policy: ImportPolicy::Strict,
                rewind: false,
                id: None,
            };
            Some(Safer::Import(Box::new(Action::Import(
                pool.clone(),
                read_only,
            ))))
        }
        Action::Import(..) => None,
        _ => Some(Safer::Clone(target.clone())),
    }
}

/// A plan of `action` on `target` with the names the input gives: what it
/// loses, its steps, who else used the pool, this host for a rewind; with
/// `refused`, what stops it and a failure.
fn hostile_plan(
    next: &mut impl FnMut() -> Untrusted<String>,
    action: &Action,
    target: &SnapshotName,
    refused: bool,
) -> Plan {
    let host = PoolHost {
        hostid: next().get().parse().ok(),
        hostname: Some(next()),
    };
    let length = u64::try_from(next().get().len()).unwrap_or(u64::MAX);
    let outcome = if refused {
        Outcome::Failed {
            what: next().into_inner(),
            why: vec![next().into_inner()],
            next: Vec::new(),
        }
    } else {
        Outcome::Done(String::new())
    };
    let this_host = matches!(action, Action::Import(_, options) if options.rewind);
    let review = Review {
        loses: hostile_losses(next, target, length),
        freed: Some(u64::MAX),
        keeps: vec![Keep::Snapshots(usize::MAX), Keep::Bookmarks(1)],
        undo: Some(Box::new(Action::Export(target.pool()))),
        safer: safer(action, target),
        stops: if refused {
            hostile_stops(next, target, &host)
        } else {
            Vec::new()
        },
        pool_use: Some(PoolUse {
            host: Some(host),
            written: Some(length.saturating_mul(1 << 40)),
            this_host,
            active: refused,
        }),
    };
    Plan {
        steps: hostile_steps(next, action, target),
        outcome,
        review,
    }
}

/// The changes reviewed: a rollback of the focused environment to the
/// snapshot the input names, and the rewind and the forced import of the
/// pool it names; each with the snapshot its plan names.
fn reviewed(
    model: &Model,
    next: &mut impl FnMut() -> Untrusted<String>,
) -> Option<(SnapshotName, Vec<Action>)> {
    let environment = model.focus.clone()?;
    let named = format!("{environment}@{}", next().get()).parse();
    let target = named
        .or_else(|_| format!("{environment}@target").parse::<SnapshotName>())
        .ok()?;
    let pool = next()
        .get()
        .parse::<PoolName>()
        .unwrap_or_else(|_| target.pool());
    let import = |rewind, policy| {
        let options = ImportOptions {
            access: Access::ReadWrite,
            policy,
            rewind,
            id: None,
        };
        Action::Import(pool.clone(), options)
    };
    let actions = vec![
        Action::Rollback(target.clone()),
        import(true, ImportPolicy::Strict),
        import(false, ImportPolicy::Force),
    ];
    Some((target, actions))
}

/// The review pages of a rollback, a rewind and a forced import with the
/// names the input gives: each while it is planned, then with a plan the
/// input names refused and not, typed in, scrolled, its focus moved,
/// drawn and said; then closed.
fn review_pages(
    mut model: Model,
    next: &mut impl FnMut() -> Untrusted<String>,
    size: (u16, u16),
) -> Model {
    let (width, height) = size;
    let Some((target, actions)) = reviewed(&model, next) else {
        return model;
    };
    for action in actions {
        let _ = follow(&mut model, Then::Review(action.clone()));
        check(&model, width, height);
        let _ = page(&model);
        for refused in [false, true] {
            let planned = Event::Planned {
                action: Box::new(action.clone()),
                plan: hostile_plan(next, &action, &target, refused),
            };
            model = update(model, Message::Event(Box::new(planned))).0;
            for key in [Key::Char('x'), Key::PageDown, Key::Tab, Key::Tab] {
                check(&model, width, height);
                let _ = page(&model);
                model = update(model, Message::Key(KeyPress::new(key))).0;
            }
        }
        model = update(model, Message::Key(KeyPress::new(Key::Escape))).0;
    }
    model
}

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
