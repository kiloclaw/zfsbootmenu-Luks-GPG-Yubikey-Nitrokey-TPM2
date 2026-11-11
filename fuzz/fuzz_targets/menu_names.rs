//! Hostile names through the menu's view and its line view (P13 of
//! docs/dev/threat-model.md): the text a pool, the files of its boot
//! environments and of the EFI system partition, crypttab and the programs
//! supply (A2, A3) reaches the screen. The input, split at its newlines,
//! gives the names and texts the hostile machine shows (`machine`): the
//! kernel versions, the command line, the LUKS volumes' names and devices,
//! the names and notes of the pools' devices, the host that used a pool
//! not imported, and the log.
//!
//! Every input draws what it always did: every screen in each key map, the
//! classic one and the workspace's; the workspace's Boot tab and help under
//! the ZFS version and host ID the input gives, start-up as its Boot tab
//! with a stage that failed for a reason the input gives, and its Log tab
//! while start-up runs; the Snapshots tab's timeline of the snapshots the
//! input names, a group folded; the review pages of a rollback, a destroy,
//! a rewind and a forced import, with their field and without it, as a dry
//! run shows them (`reviews`); and the Pools tab's tree and a device's
//! kernel messages (`pools`).
//!
//! The first byte picks the size, 52×20, 80×25, 132×43 or 80×24, the
//! theme, in colour or ASCII, and one group more, drawn in both key maps
//! where a view is in both. The group takes the input's lines again from
//! the first: environments and a pool they name join the machine, then
//! the group's names follow.
//!
//! - `screens`: every screen and the help of every view, start-up among
//!   them; the classic Snapshots and Pools views with the input's names;
//! - `changes`: the differences since a snapshot, its files and packages,
//!   the restore of paths the input gives, and of none;
//! - `history`: the boot history the ESP gives, and start-up's events;
//! - `questions`: the unlock dialogs, start-up's questions, a pool that
//!   did not import, a TPM2 key that changed and its re-seal, and how
//!   actions end, failures included;
//! - `sheets`: the choices of a snapshot, of each pool and of the power,
//!   every choice followed: the classic confirmations, the names asked
//!   for; then the palette: its history (`Ctrl-R`) and `Tab`, the first
//!   entry of each section and of each kind followed into its dry run,
//!   whose plan the input names, the save editor of the machine's
//!   command line and of lines the machine refuses, and the palette of
//!   the locked menu, its marks drawn, and a dry run under the lock, its
//!   button marked.
//!
//! Drawing may not panic, and no cell may hold a character that acts on a
//! terminal, nor any line the line view says.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use zbm_display::keys::{Key, KeyPress};
use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::keymap::{Context, Map};
use zbm_tui::menu::lines::page;
use zbm_tui::menu::model::{Editor, Model, Screen};
use zbm_tui::menu::theme::Theme;
use zbm_tui::menu::view::view;
use zbm_tui::menu::world::{Event, Stage, StageState};
use zbm_tui::menu::{Message, update};
use zbm_types::{DatasetName, Untrusted, must_escape};

#[path = "menu_names/dialogs.rs"]
mod dialogs;
#[path = "menu_names/machine.rs"]
mod machine;
#[path = "menu_names/pools.rs"]
mod pools;
#[path = "menu_names/reviews.rs"]
mod reviews;
#[path = "menu_names/screens.rs"]
mod screens;

/// The sizes drawn: the smallest the menu lays out for, the console's, a
/// large one, wide enough for the host ID and the time, and the
/// workspace's smallest console, last so that a first byte picks the size
/// it did before.
const SIZES: [(u16, u16); 4] = [(52, 20), (80, 25), (132, 43), (80, 24)];

/// The themes drawn: in colour, and in ASCII.
const THEMES: [Capabilities; 2] = [Capabilities::FULL, Capabilities::ASCII];

/// The key maps.
const MAPS: [Map; 2] = [Map::Classic, Map::Workspace];

/// The screens every input draws.
const SCREENS: [Screen; 7] = [
    Screen::Main,
    Screen::Snapshots,
    Screen::Kernels,
    Screen::CommandLine,
    Screen::Pools,
    Screen::Recovery,
    Screen::Log,
];

/// The groups of views an input draws besides, one of which the first
/// byte picks.
#[derive(Clone, Copy)]
enum Group {
    Screens,
    Changes,
    History,
    Questions,
    Sheets,
}

/// The groups, in the order the first byte counts them.
const GROUPS: [Group; 5] = [
    Group::Screens,
    Group::Changes,
    Group::History,
    Group::Questions,
    Group::Sheets,
];

/// Draws `model` at `size`, and checks every cell.
fn check(model: &Model, size: (u16, u16)) {
    let (width, height) = size;
    let Ok(mut terminal) = Terminal::new(TestBackend::new(width, height));
    if terminal.draw(|frame| view(model, frame)).is_err() {
        return;
    }
    for cell in terminal.backend().buffer().content() {
        let symbol = cell.symbol();
        assert!(!symbol.contains(must_escape), "{symbol:?} drawn");
    }
}

/// Draws `model` at `size` and says it as the line view does, and checks
/// every cell and every line.
fn check_page(model: &Model, size: (u16, u16)) {
    check(model, size);
    for line in page(model) {
        assert!(!line.contains(must_escape), "{line:?} said");
    }
}

/// `model` after its machine sends `event`.
fn sent(model: Model, event: Event) -> Model {
    update(model, Message::Event(Box::new(event))).0
}

/// `model` after `key` is pressed.
fn press(model: Model, key: Key) -> Model {
    update(model, Message::Key(KeyPress::new(key))).0
}

/// The menu of `model`, focused on the environment it selects.
fn focused(mut model: Model) -> Option<(Model, DatasetName)> {
    let name = model.selected_environment()?.name.clone();
    model.focus = Some(name.clone());
    Some((model, name))
}

fuzz_target!(|data: &[u8]| {
    let Some((&first, data)) = data.split_first() else {
        return;
    };
    let first = usize::from(first);
    let size = SIZES[first % SIZES.len()];
    let theme = Theme::new(THEMES[first / SIZES.len() % THEMES.len()]);
    let group = GROUPS[first / (SIZES.len() * THEMES.len()) % GROUPS.len()];
    let text = String::from_utf8_lossy(data);
    let lines = || text.split('\n').map(|part| Untrusted::new(part.to_owned()));
    let mut parts = lines();
    let mut next = || parts.next().unwrap_or_default();

    let snapshot = machine::hostile(&mut next);
    let Some((model, name)) = focused(Model::new(snapshot.clone(), theme)) else {
        return;
    };
    every_input(model, &name, &mut next, size);
    // The group takes the input's lines again from the first, so that it
    // has the input's names however many the draws above took: first the
    // names of the environments and the pool it adds.
    let mut parts = lines();
    let mut next = || parts.next().unwrap_or_default();
    let mut snapshot = snapshot;
    machine::added(&mut snapshot, &mut next);
    let Some((model, _)) = focused(Model::new(snapshot, theme)) else {
        return;
    };
    match group {
        Group::Screens => screens::screens(model, &mut next, size),
        Group::Changes => screens::changes(model, &mut next, size),
        Group::History => screens::history(model, &mut next, size),
        Group::Questions => dialogs::questions(model, &mut next, size),
        Group::Sheets => dialogs::sheets(model, &mut next, size),
    }
});

/// What every input draws: every screen in each key map, the Boot tab,
/// start-up, the Snapshots tab, the review pages and the Pools tab.
fn every_input(
    mut model: Model,
    name: &DatasetName,
    next: &mut impl FnMut() -> Untrusted<String>,
    size: (u16, u16),
) {
    for map in MAPS {
        model.map = map;
        model.refresh();
        for screen in SCREENS {
            model.screen = screen;
            model.editor = (screen == Screen::CommandLine).then(|| editor(name, next));
            check_page(&model, size);
        }
    }
    boot_tab(&mut model, next, size);
    let model = snapshots_tab(model, name, next, size);
    let model = reviews::review_pages(model, next, size);
    pools::pools_tab(model, next, size);
}

/// The command line editor of `environment`, with a kernel and a text the
/// input gives.
fn editor(environment: &DatasetName, next: &mut impl FnMut() -> Untrusted<String>) -> Editor {
    Editor {
        environment: environment.clone(),
        kernel: Some(next()),
        text: next().into_inner(),
        save: false,
    }
}

/// The workspace's Boot tab, with what needs attention, its kernels
/// column under the path, with the inspector that wraps, and its help,
/// under the ZFS version; a host ID that is not one is none. Then
/// start-up as the Boot tab, and the Log tab while it runs.
fn boot_tab(model: &mut Model, next: &mut impl FnMut() -> Untrusted<String>, size: (u16, u16)) {
    model.snapshot.zfs_version = Some(next());
    model.snapshot.host = next().parse().ok();
    model.map = Map::Workspace;
    model.editor = None;
    model.refresh();
    for screen in [Screen::Main, Screen::Kernels, Screen::Help(Context::Main)] {
        model.screen = screen;
        check_page(model, size);
    }
    let kept = std::mem::replace(&mut model.snapshot.startup, stages(next));
    for screen in [Screen::Startup, Screen::Log] {
        model.screen = screen;
        check_page(model, size);
    }
    model.snapshot.startup = kept;
}

/// Start-up's stages: one failed for a reason the input gives, one
/// running, both named by it.
fn stages(next: &mut impl FnMut() -> Untrusted<String>) -> Vec<Stage> {
    vec![
        Stage {
            name: next().into_inner(),
            state: StageState::Failed(next().into_inner()),
        },
        Stage {
            name: next().into_inner(),
            state: StageState::Running,
        },
    ]
}

/// The workspace's Snapshots tab: the snapshots the input names, taken at
/// times that cross the boundaries of the groups, as the machine sends
/// them, so that the timeline groups them anew; then one group folded.
fn snapshots_tab(
    mut model: Model,
    name: &DatasetName,
    next: &mut impl FnMut() -> Untrusted<String>,
    size: (u16, u16),
) -> Model {
    let loaded = machine::taken(name, model.snapshot.now, next);
    model.focus = Some(name.clone());
    model.screen = Screen::Snapshots;
    let model = sent(model, loaded);
    check_page(&model, size);
    let model = press(model, Key::Char('z'));
    check_page(&model, size);
    model
}
