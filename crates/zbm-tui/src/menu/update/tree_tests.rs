//! Tests of the workspace's Pools tab as a tree (ADR 0016, point 9):
//! `Enter` opens a pool's details on a pool and on its vdev, asks for a
//! device's kernel messages on a device nothing is below, which then show
//! until Back, and opens a volume; the keys of another kind of row are not
//! shown; the line view prints the tree indented and numbers its stops
//! alone, as the arrows go; and the current menu is unchanged.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};
use zbm_types::Untrusted;

use super::{Message, update};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::keymap::{self, Context, Intent, Map};
use crate::menu::lines::{self, Typed};
use crate::menu::model::{Dialog, Model, Screen};
use crate::menu::theme::Theme;
use crate::menu::world::Action;
use crate::text::text;

/// The demo in the workspace, started, its countdown stopped, on the
/// Pools tab, after `downs` presses of Down.
fn tab(downs: usize) -> Session {
    let options = DemoOptions {
        workspace: true,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char('3'));
    for _ in 0..downs {
        session.key(Key::Down);
    }
    session
}

/// What `Enter` asks the machine for on the row `downs` below the first,
/// and the menu after it.
fn enter(downs: usize) -> (Model, Vec<Action>) {
    let model = tab(downs).into_model();
    update(model, Message::Key(KeyPress::new(Key::Enter)))
}

/// The title of the dialog open, if one is.
fn dialog_title(model: &Model) -> Option<&str> {
    match &model.dialog {
        Some(Dialog::Choose { title, .. }) => Some(title.as_str()),
        _ => None,
    }
}

#[test]
fn enter_opens_a_pools_details_on_it_and_its_vdevs() {
    // zroot, then its mirror.
    for downs in [0, 1] {
        let (model, actions) = enter(downs);
        assert_eq!(actions, []);
        assert_eq!(
            dialog_title(&model),
            Some(text!("pool-title", pool = "zroot")).as_deref()
        );
    }
    let (model, _) = enter(8);
    assert_eq!(
        dialog_title(&model),
        Some(text!("pool-title", pool = "tank")).as_deref()
    );
}

#[test]
fn enter_on_a_device_asks_for_its_messages_by_the_path_it_has() {
    let (model, actions) = enter(10);

    let device = Action::LoadDeviceLog(Untrusted::new("/dev/sdb2".to_owned()));
    assert_eq!(actions, [device]);
    assert_eq!(model.dialog, None);
    // A device straight below its pool, as a cache can be.
    let (_, actions) = enter(4);
    let cache = Action::LoadDeviceLog(Untrusted::new("/dev/nvme2n1".to_owned()));
    assert_eq!(actions, [cache]);
}

#[test]
fn enter_on_a_volume_opens_it() {
    let (_, actions) = enter(12);

    assert!(
        matches!(actions.as_slice(), [Action::Open { device, .. }] if device == "/dev/sda2"),
        "{actions:?}"
    );
}

#[test]
fn a_devices_messages_show_below_the_tree_until_back() {
    let mut session = tab(10);
    session.key(Key::Enter);

    let model = session.model();
    let log = model
        .device_log
        .as_ref()
        .map(|log| log.device.get().as_str());
    assert_eq!(log, Some("/dev/sdb2"));
    assert_eq!(model.path(), ["tank", "raidz1-0", "sdb2"]);
    // The line view says them, unnumbered, after the tree.
    let page = lines::page(model);
    let title = text!("device-log-title", device = "sdb2");
    let at = page.iter().position(|line| *line == format!("  {title}"));
    assert!(at.is_some(), "{page:#?}");
    assert!(
        page.iter()
            .any(|line| line.ends_with("Synchronizing SCSI cache"))
    );

    session.key(Key::Escape);
    assert_eq!(session.model().device_log, None);
    assert_eq!(session.model().screen, Screen::Pools);
    session.key(Key::Escape);
    assert_eq!(session.model().screen, Screen::Main);
}

#[test]
fn the_line_view_prints_the_tree_indented_and_numbers_its_stops() {
    let mut session = tab(0);

    let page = lines::page(session.model());
    let tree: Vec<&str> = page[1..15].iter().map(String::as_str).collect();
    assert_eq!(
        tree,
        [
            "  1) zroot, read-only, ONLINE",
            "  2)   mirror-0, ONLINE",
            "  3)     nvme0n1p2, ONLINE",
            "  4)     nvme1n1p2, ONLINE",
            "    cache",
            "  5)     nvme2n1, ONLINE",
            "  6)   luks-zroot-a, Volume, open",
            "  7)   luks-zroot-b, Volume, open",
            "  8) tank, read-only, DEGRADED",
            "  9)   raidz1-0, DEGRADED",
            "  10)     sda2, ONLINE",
            "  11)     sdb2, cannot open, UNAVAIL",
            "  12)     sdc2, too many errors, DEGRADED",
            "  13)   luks-tank, Volume, open",
        ]
    );
    // A number chooses the row the arrows reach as many rows down.
    let Typed::Keys(keys) = lines::typed("11", Context::Pools, Map::Workspace) else {
        panic!("not keys");
    };
    for key in keys {
        session.press(key);
    }
    let model = session.model();
    assert_eq!(model.path(), ["tank", "raidz1-0", "sdb2"]);
    assert!(model.device_log.is_some());
}

#[test]
fn the_header_of_the_volumes_of_no_pool_is_neither_numbered_nor_a_stop() {
    let mut model = tab(0).into_model();
    let mut snapshot = model.snapshot.clone();
    for volume in &mut snapshot.volumes {
        volume.pool = None;
    }
    model.replace_snapshot(snapshot);

    let page = lines::page(&model);
    let header = page.iter().position(|line| line == "  No pool").unwrap();
    assert_eq!(
        page[header - 1],
        "  10)     sdc2, too many errors, DEGRADED"
    );
    assert_eq!(page[header + 1], "  11)   luks-zroot-a, Volume, open");
    let mut model = model;
    for _ in 0..10 {
        model = update(model, Message::Key(KeyPress::new(Key::Down))).0;
    }
    assert_eq!(
        model.path(),
        [text!("pools-no-pool"), "luks-zroot-a".to_owned()]
    );
}

#[test]
fn the_page_keys_home_and_end_go_over_the_stops_of_the_tree_alone() {
    let mut model = tab(0).into_model();
    let mut snapshot = model.snapshot.clone();
    for volume in &mut snapshot.volumes {
        volume.pool = None;
    }
    model.replace_snapshot(snapshot);
    let press = |model: Model, key: Key| update(model, Message::Key(KeyPress::new(key))).0;
    let no_pool = text!("pools-no-pool");

    let model = press(model, Key::End);
    assert_eq!(model.path(), [no_pool.clone(), "luks-tank".to_owned()]);
    let mut model = press(model, Key::Home);
    assert_eq!(model.path(), ["zroot"]);
    // Down a page at a time to the end, and up again: never on the header.
    let mut seen = Vec::new();
    for key in [Key::PageDown; 4].into_iter().chain([Key::PageUp; 4]) {
        model = press(model, key);
        let node = model.selected_node().expect("a row is selected");
        assert!(node.is_stop(), "{key:?} stopped on {}", node.name);
        seen.push(model.path());
    }
    assert!(seen.contains(&vec![no_pool, "luks-tank".to_owned()]));
    assert_eq!(model.path(), ["zroot"]);
    // Up from the first volume of no pool passes over its header.
    let model = press(press(model, Key::End), Key::Up);
    let model = press(press(model, Key::Up), Key::Up);
    assert_eq!(model.path(), ["tank", "raidz1-0", "sdc2"]);
}

#[test]
fn a_rows_toolbar_shows_the_keys_of_its_kind_alone() {
    let shown = |downs: usize, intent: Intent| {
        let model = tab(downs).into_model();
        keymap::is_shown(Context::Pools, intent, &model)
    };

    // A pool, a vdev and a device: their pool's keys.
    for downs in [0, 1, 2] {
        assert!(shown(downs, Intent::Import), "{downs}");
        assert!(shown(downs, Intent::ReadWrite), "{downs}");
        assert!(!shown(downs, Intent::Unlock), "{downs}");
    }
    // A volume: its own.
    assert!(shown(5, Intent::Unlock));
    assert!(!shown(5, Intent::Import));
    assert!(!shown(5, Intent::Rewind));
}

#[test]
fn enter_says_what_it_does_on_each_kind_of_row() {
    let said = |downs: usize| {
        let model = tab(downs).into_model();
        keymap::label(Context::Pools, Intent::Primary, &model)
    };

    // A pool, a vdev, a device and a volume.
    assert_eq!(said(0), keymap::describe(Context::Pools, Intent::Primary));
    assert_eq!(said(1), keymap::describe(Context::Pools, Intent::Primary));
    assert_eq!(said(2), crate::text::id!("action-messages"));
    assert_eq!(said(5), keymap::describe(Context::Pools, Intent::Primary));
}

#[test]
fn the_current_menu_keeps_its_list_and_its_keys() {
    let options = DemoOptions::default();
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    session.key(Key::Char('p'));
    session.key(Key::Down);
    let model = session.into_model();

    assert_eq!(model.pools.matches.len(), 5);
    assert!(keymap::is_shown(Context::Pools, Intent::Unlock, &model));
    let (model, actions) = update(model, Message::Key(KeyPress::new(Key::Enter)));
    assert_eq!(actions, []);
    assert_eq!(
        dialog_title(&model),
        Some(text!("pool-title", pool = "tank")).as_deref()
    );
}
