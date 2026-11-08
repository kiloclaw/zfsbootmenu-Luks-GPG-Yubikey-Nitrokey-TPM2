//! The views: every screen and the help of every view, the differences
//! since a snapshot, and the boot history; with the names the input gives.

use zbm_display::keys::Key;
use zbm_tui::menu::keymap::{Context, Map};
use zbm_tui::menu::model::{Model, Screen};
use zbm_tui::menu::world::{
    Change, ChangeKind, Event, History, PackageChange, PackageDiff, PackageKind, Recorded,
    SavedBoot, StageState, UnlockOutcome,
};
use zbm_types::{BootRecord, DatasetName, SnapshotName, Untrusted};

use super::{MAPS, check_page, editor, machine, pools, press, sent, stages};

/// The views of each key map that have no help of their own.
const VIEWS: [Screen; 9] = [
    Screen::Startup,
    Screen::Main,
    Screen::Snapshots,
    Screen::Kernels,
    Screen::CommandLine,
    Screen::Pools,
    Screen::Recovery,
    Screen::Log,
    Screen::Diff,
];

/// Every context that has a help page.
const CONTEXTS: [Context; 13] = [
    Context::Startup,
    Context::Main,
    Context::Snapshots,
    Context::Kernels,
    Context::Pools,
    Context::Recovery,
    Context::Log,
    Context::Help,
    Context::Diff,
    Context::Packages,
    Context::Choose,
    Context::Text,
    Context::Review,
];

/// Every screen and the help of every view in each key map, while start-up
/// runs a stage and another failed, both named by the input; then the
/// classic map's Snapshots view with the snapshots the input names, and
/// unread, and its Pools view after a scan found the input's pools.
pub fn screens(mut model: Model, next: &mut impl FnMut() -> Untrusted<String>, size: (u16, u16)) {
    let Some(name) = model.focus.clone() else {
        return;
    };
    model.snapshot.startup.extend(stages(next));
    for map in MAPS {
        model.map = map;
        model.refresh();
        for screen in VIEWS.into_iter().chain(CONTEXTS.map(Screen::Help)) {
            model.screen = screen;
            model.editor = (screen == Screen::CommandLine).then(|| editor(&name, next));
            check_page(&model, size);
        }
    }
    model.editor = None;
    model.map = Map::Classic;
    model.screen = Screen::Snapshots;
    let loaded = machine::taken(&name, model.snapshot.now, next);
    let mut model = sent(model, loaded);
    check_page(&model, size);
    model = sent(model, Event::SnapshotsUnread(name));
    check_page(&model, size);
    model.screen = Screen::Pools;
    let mut model = sent(model, Event::PoolsScanned(pools::hostile_pools(next)));
    for _ in 0..model.pools.matches.len() {
        check_page(&model, size);
        model = press(model, Key::Down);
    }
}

/// The paths that changed since a snapshot, one of each kind, named by the
/// input, with the lines of their text diffs.
fn changed(next: &mut impl FnMut() -> Untrusted<String>) -> Vec<Change> {
    [
        ChangeKind::Removed,
        ChangeKind::Modified,
        ChangeKind::Renamed,
        ChangeKind::Created,
    ]
    .into_iter()
    .map(|kind| Change {
        kind,
        path: next(),
        renamed_to: (kind == ChangeKind::Renamed).then(&mut *next),
        preview: vec![next(), next()],
    })
    .collect()
}

/// The packages that changed, one of each kind, their manager, names,
/// versions and files given by the input.
fn packages(next: &mut impl FnMut() -> Untrusted<String>) -> PackageDiff {
    let kinds = [
        PackageKind::Installed,
        PackageKind::Removed,
        PackageKind::Upgraded,
        PackageKind::Downgraded,
    ];
    PackageDiff {
        manager: next().into_inner(),
        changes: kinds
            .into_iter()
            .map(|kind| PackageChange {
                name: next().into_inner(),
                kind,
                before: Some(next().into_inner()),
                after: Some(next().into_inner()),
                files: vec![next().into_inner(), next().into_inner()],
            })
            .collect(),
    }
}

/// The differences since a snapshot the input names, in each key map:
/// against the live filesystem, two paths marked, the packages and the
/// files of the first shown, and the restore of the marked paths asked
/// for; again, the path created selected, which restores nothing; then
/// against another snapshot it names, where nothing restores.
pub fn changes(mut model: Model, next: &mut impl FnMut() -> Untrusted<String>, size: (u16, u16)) {
    let Some(environment) = model.focus.clone() else {
        return;
    };
    let (Some(from), Some(other)) = (snapshot(&environment, next), snapshot(&environment, next))
    else {
        return;
    };
    let (changes, packages) = (changed(next), packages(next));
    let restore = [
        Key::Home,
        Key::Char(' '),
        Key::Down,
        Key::Char(' '),
        Key::Right,
        Key::Enter,
        Key::Enter,
    ];
    let created = [Key::End, Key::Enter];
    let passes = [
        (None, restore.as_slice()),
        (None, created.as_slice()),
        (Some(other), restore.as_slice()),
    ];
    for map in MAPS {
        model.map = map;
        for (to, keys) in &passes {
            let ready = Event::DiffReady {
                from: from.clone(),
                to: to.clone(),
                changes: changes.clone(),
                packages: Some(packages.clone()),
            };
            model = sent(model, ready);
            for key in keys.iter().copied() {
                check_page(&model, size);
                model = press(model, key);
            }
            check_page(&model, size);
            model.dialog = None;
            model.notification = None;
        }
    }
}

/// A snapshot of `environment` the input names, else one named `diff`.
fn snapshot(
    environment: &DatasetName,
    next: &mut impl FnMut() -> Untrusted<String>,
) -> Option<SnapshotName> {
    format!("{environment}@{}", next().get())
        .parse()
        .or_else(|_| format!("{environment}@diff").parse())
        .ok()
}

/// The record of a boot of `environment`, with the paths, command line and
/// loader the input gives.
fn record(environment: String, next: &mut impl FnMut() -> Untrusted<String>) -> BootRecord {
    BootRecord {
        time: u64::MAX,
        environment: Untrusted::new(environment),
        kernel: next(),
        initramfs: next(),
        command_line: next(),
        devicetree: Some(next()),
        loaded_by: next(),
        measured: true,
        command_line_edited: true,
    }
}

/// The boot histories an ESP the input writes gives: saved boots named by
/// it, with its files and records, of `environment` and of one it names,
/// one of them unreadable for a reason it gives; one that cannot be read;
/// none.
fn histories(
    environment: &DatasetName,
    next: &mut impl FnMut() -> Untrusted<String>,
) -> [History; 3] {
    let mut boots = Vec::new();
    for recorded in 0..4 {
        let name = next();
        let files = vec![(next(), u64::MAX), (next(), 0)];
        let record = match recorded {
            0 => Recorded::Read(record(environment.to_string(), next)),
            1 => Recorded::Read(record(next().into_inner(), next)),
            2 => Recorded::Unreadable(next().into_inner()),
            _ => Recorded::Missing,
        };
        boots.push(SavedBoot {
            name,
            files,
            record,
        });
    }
    let why = next().into_inner();
    [
        History::Saved(boots),
        History::Unavailable {
            why,
            code: u16::MAX,
        },
        History::Off,
    ]
}

/// The boot history in each key map's Log view, and the Boot tab's
/// inspector, which says when the environment was last booted; then
/// start-up's stages and unlocking as their events say, named by the input.
pub fn history(mut model: Model, next: &mut impl FnMut() -> Untrusted<String>, size: (u16, u16)) {
    let Some(environment) = model.focus.clone() else {
        return;
    };
    for history in histories(&environment, next) {
        for map in MAPS {
            model.map = map;
            model = sent(model, Event::HistoryLoaded(history.clone()));
            for screen in [Screen::Log, Screen::Main] {
                model.screen = screen;
                check_page(&model, size);
            }
        }
    }
    let events = [
        Event::StageStarted(next().into_inner()),
        Event::StageFinished {
            stage: next().into_inner(),
            state: StageState::Failed(next().into_inner()),
        },
        Event::UnlockFinished {
            volume: next().into_inner(),
            outcome: UnlockOutcome::Opened,
        },
        Event::UnlockFinished {
            volume: next().into_inner(),
            outcome: UnlockOutcome::Skipped,
        },
        Event::UnlockFinished {
            volume: next().into_inner(),
            outcome: UnlockOutcome::Failed(next().into_inner()),
        },
    ];
    for event in events {
        model = sent(model, event);
        for map in MAPS {
            model.map = map;
            for screen in [Screen::Startup, Screen::Log] {
                model.screen = screen;
                check_page(&model, size);
            }
        }
    }
}
