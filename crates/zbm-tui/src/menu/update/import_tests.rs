//! The ways the menu imports a pool (ADR 0016, point 3): one that is
//! imported read-only or read/write again, or rewound to its checkpoint;
//! one that is not as this host, as the host that owns it, or forced.

use pretty_assertions::assert_eq;
use zbm_types::ImportPolicy;

use super::{dialogs, events, screens};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine};
use crate::menu::keymap::Intent;
use crate::menu::model::{Choice, Dialog, Model, Screen, Then};
use crate::menu::theme::Theme;
use crate::menu::world::{Access, Action, Event, ImportOptions, Offer, Outcome};

/// The menu on the demo machine, offering everything, on the pools view:
/// `zroot` imported read-only, `tank` read/write with a checkpoint, and
/// `backup`, of another host, not imported.
fn pools() -> Model {
    let options = DemoOptions {
        foreign: true,
        ..DemoOptions::default()
    };
    let mut snapshot = FakeMachine::new(&options).snapshot().clone();
    snapshot.offers = Offer::ALL.into_iter().collect();
    if let Some(tank) = snapshot
        .pools
        .iter_mut()
        .find(|p| p.name.as_str() == "tank")
    {
        tank.writable = true;
    }
    let mut model = Model::new(snapshot, Theme::new(Capabilities::FULL));
    events::start(&mut model);
    model.countdown = None;
    model.screen = Screen::Pools;
    model
}

/// `model` with the pool `name` selected.
fn select(mut model: Model, name: &str) -> Model {
    let row = model
        .snapshot
        .pools
        .iter()
        .position(|pool| pool.name.as_str() == name)
        .unwrap();
    model.pools.selected = row;
    model
}

fn import(pool: &str, access: Access, policy: ImportPolicy, rewind: bool) -> Action {
    let options = ImportOptions {
        access,
        policy,
        rewind,
        id: None,
    };
    Action::Import(pool.parse().unwrap(), options)
}

/// The actions of the choices `model` shows, confirmed.
fn choices(model: &Model) -> Vec<Action> {
    let Some(Dialog::Choose { choices, .. }) = &model.dialog else {
        panic!("no choices: {:?}", model.dialog);
    };
    choices.iter().map(action).collect()
}

fn action(choice: &Choice) -> Action {
    match &choice.then {
        Then::Act(action) | Then::Confirm { action, .. } => action.clone(),
        other => panic!("not an import: {other:?}"),
    }
}

#[test]
fn an_imported_pool_is_imported_with_the_other_access_or_rewound() {
    let mut zroot = select(pools(), "zroot");
    assert_eq!(screens::intent(&mut zroot, Intent::Import), []);
    assert_eq!(
        choices(&zroot),
        [import(
            "zroot",
            Access::ReadWrite,
            ImportPolicy::Strict,
            false
        )]
    );

    let mut tank = select(pools(), "tank");
    assert_eq!(screens::intent(&mut tank, Intent::Import), []);
    assert_eq!(
        choices(&tank),
        [
            import("tank", Access::ReadOnly, ImportPolicy::Strict, false),
            import("tank", Access::ReadWrite, ImportPolicy::Strict, true),
        ]
    );
}

#[test]
fn a_pool_not_imported_is_imported_as_this_host_as_its_own_or_forced() {
    let mut backup = select(pools(), "backup");
    assert_eq!(screens::intent(&mut backup, Intent::Import), []);
    assert_eq!(
        choices(&backup),
        [
            import("backup", Access::ReadOnly, ImportPolicy::Strict, false),
            import("backup", Access::ReadOnly, ImportPolicy::Hostid, false),
            import("backup", Access::ReadOnly, ImportPolicy::Force, false),
        ]
    );
    // The forced import is confirmed by the pool's name.
    let forced = match &backup.dialog {
        Some(Dialog::Choose { choices, .. }) => choices.get(2).map(|choice| &choice.then),
        _ => None,
    };
    assert!(matches!(forced, Some(Then::Confirm { name, .. }) if name == "backup"));
}

#[test]
fn the_keys_import_read_write_and_confirm_the_rewind() {
    let mut zroot = select(pools(), "zroot");
    assert_eq!(
        screens::intent(&mut zroot, Intent::ReadWrite),
        [import(
            "zroot",
            Access::ReadWrite,
            ImportPolicy::Strict,
            false
        )]
    );

    let mut tank = select(pools(), "tank");
    assert_eq!(screens::intent(&mut tank, Intent::Rewind), []);
    let Some(Dialog::Confirm { action, name, .. }) = &tank.dialog else {
        panic!("no confirmation: {:?}", tank.dialog);
    };
    assert_eq!(
        (action, name.as_str()),
        (
            &import("tank", Access::ReadWrite, ImportPolicy::Strict, true),
            "tank"
        )
    );

    // zroot has no checkpoint: nothing to confirm, and the menu says so.
    let mut zroot = select(pools(), "zroot");
    assert_eq!(screens::intent(&mut zroot, Intent::Rewind), []);
    assert!(zroot.dialog.is_none());
    assert!(matches!(
        &zroot.notification,
        Some(Outcome::Warning(text)) if text.contains("zroot has no checkpoint")
    ));

    // Whether `backup`, not imported, has one is not known: the machine
    // checks once it is imported.
    let mut backup = select(pools(), "backup");
    assert_eq!(
        backup.selected_pool().map(|pool| pool.checkpoint),
        Some(None)
    );
    assert_eq!(screens::intent(&mut backup, Intent::Rewind), []);
    assert!(matches!(&backup.dialog, Some(Dialog::Confirm { .. })));
}

#[test]
fn the_details_of_a_pool_offer_its_imports_and_its_export() {
    let mut model = pools();
    let tank = model.snapshot.pools[1].clone();
    assert_eq!(tank.name.as_str(), "tank");
    dialogs::pool_details(&mut model, &tank);
    let tank_choices = [
        import("tank", Access::ReadOnly, ImportPolicy::Strict, false),
        Action::Export("tank".parse().unwrap()),
        import("tank", Access::ReadWrite, ImportPolicy::Strict, true),
    ];
    assert_eq!(choices(&model), tank_choices);

    // The workspace offers each disk's kernel messages too, by the path
    // the pool names it by.
    model.map = crate::menu::keymap::Map::Workspace;
    dialogs::pool_details(&mut model, &tank);
    let device = |path: &str| Action::LoadDeviceLog(zbm_types::Untrusted::new(path.to_owned()));
    let mut workspace = tank_choices.to_vec();
    workspace.extend(["/dev/sda2", "/dev/sdb2", "/dev/sdc2"].map(device));
    assert_eq!(choices(&model), workspace);
    // Without the export offered, the rest stays.
    model.snapshot.offers.remove(&Offer::Export);
    dialogs::pool_details(&mut model, &tank);
    assert!(!choices(&model).contains(&Action::Export("tank".parse().unwrap())));
    model.map = crate::menu::keymap::Map::Classic;

    let backup = model.snapshot.pools[2].clone();
    assert_eq!(backup.name.as_str(), "backup");
    dialogs::pool_details(&mut model, &backup);
    assert_eq!(
        choices(&model),
        [
            import("backup", Access::ReadOnly, ImportPolicy::Hostid, false),
            import("backup", Access::ReadOnly, ImportPolicy::Force, false),
        ]
    );
}

/// The menu on the demo machine once its scan found the pools start-up left
/// alone: `archive`, of another host, and a `tank` that is not the
/// imported one.
fn scanned() -> Model {
    let options = DemoOptions {
        exported: true,
        ..DemoOptions::default()
    };
    let mut machine = FakeMachine::new(&options);
    let mut model = Model::new(machine.snapshot().clone(), Theme::new(Capabilities::FULL));
    for event in machine.apply(Action::ScanPools) {
        events::apply(&mut model, event);
    }
    model.snapshot.offers = Offer::ALL.into_iter().collect();
    events::start(&mut model);
    model.countdown = None;
    model.screen = Screen::Pools;
    model
}

/// `model` with the pool of the identifier `id` selected.
fn select_id(mut model: Model, id: u64) -> Model {
    let row = model
        .snapshot
        .pools
        .iter()
        .position(|pool| pool.id == Some(id))
        .unwrap();
    model.pools.selected = row;
    model
}

#[test]
fn the_scan_lists_the_pools_start_up_left_and_their_hosts() {
    let model = scanned();

    assert_eq!(
        model.pool_rows()[..4],
        [
            "zroot, ONLINE, read-only",
            "tank (id 2), DEGRADED, read-only",
            "archive, ONLINE, not imported, host backup-server (deadbeef)",
            "tank (id 5), ONLINE, not imported",
        ]
    );
    // The details say which of the two it is.
    let mut other = select_id(model, 5);
    let pool = other.selected_pool().unwrap().clone();
    dialogs::pool_details(&mut other, &pool);
    let Some(Dialog::Choose { lines, .. }) = &other.dialog else {
        panic!("no details: {:?}", other.dialog);
    };
    assert_eq!(lines[0], "tank (id 5): ONLINE · not imported");
}

#[test]
fn a_pool_named_like_an_imported_one_is_shown_but_not_offered() {
    let mut other = select_id(scanned(), 5);

    assert_eq!(screens::intent(&mut other, Intent::Import), []);
    assert!(other.dialog.is_none());
    assert!(matches!(
        &other.notification,
        Some(Outcome::Warning(text)) if text.contains("A pool named tank is imported")
    ));
    assert_eq!(screens::intent(&mut other, Intent::ReadWrite), []);
    let pool = other.selected_pool().unwrap().clone();
    dialogs::pool_details(&mut other, &pool);
    let Some(Dialog::Choose { lines, choices, .. }) = &other.dialog else {
        panic!("no details: {:?}", other.dialog);
    };
    assert!(choices.is_empty());
    assert!(lines.iter().any(|line| line.contains("export it")));
}

#[test]
fn the_imported_pool_of_a_shared_name_is_imported_again_by_its_identifier() {
    let mut tank = select_id(scanned(), 2);
    let read_write = screens::intent(&mut tank, Intent::ReadWrite);
    let mut zroot = select(scanned(), "zroot");
    let zroot_read_write = screens::intent(&mut zroot, Intent::ReadWrite);

    let by_id = |access, rewind| {
        let options = ImportOptions {
            access,
            policy: ImportPolicy::Strict,
            rewind,
            id: Some(2),
        };
        Action::Import("tank".parse().unwrap(), options)
    };
    assert_eq!(read_write, [by_id(Access::ReadWrite, false)]);
    screens::intent(&mut tank, Intent::Import);
    assert_eq!(
        choices(&tank),
        [
            by_id(Access::ReadWrite, false),
            by_id(Access::ReadWrite, true)
        ]
    );
    // A name no other pool has names its pool.
    assert_eq!(
        zroot_read_write,
        [import(
            "zroot",
            Access::ReadWrite,
            ImportPolicy::Strict,
            false
        )]
    );
}

#[test]
fn pools_not_imported_of_one_name_are_each_imported_by_their_identifier() {
    let mut model = scanned();
    model
        .snapshot
        .pools
        .retain(|pool| pool.name.as_str() != "tank" || pool.id == Some(5));
    let mut twin = model.snapshot.pools.last().unwrap().clone();
    twin.id = Some(6);
    model.snapshot.pools.push(twin);
    model.refresh();

    let mut first = select_id(model, 5);
    screens::intent(&mut first, Intent::Import);

    let ids: Vec<Option<u64>> = choices(&first)
        .into_iter()
        .map(|action| match action {
            Action::Import(_, options) => options.id,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(ids, [Some(5), Some(5), Some(5)]);
}

#[test]
fn the_scan_keeps_the_row_selected_in_the_pools_view() {
    let options = DemoOptions {
        exported: true,
        ..DemoOptions::default()
    };
    let mut machine = FakeMachine::new(&options);
    let mut model = Model::new(machine.snapshot().clone(), Theme::new(Capabilities::FULL));
    events::start(&mut model);
    model.screen = Screen::Pools;
    model.pools.selected = model.snapshot.pools.len();
    let volume = model.selected_volume().map(|volume| volume.name.clone());
    assert!(volume.is_some());

    for event in machine.apply(Action::ScanPools) {
        events::apply(&mut model, event);
    }

    // The pools the scan found come before the volumes: the volume stays
    // selected, not the pool now in its row.
    assert!(model.snapshot.pools.iter().any(|pool| !pool.imported));
    assert_eq!(
        model.selected_volume().map(|volume| volume.name.clone()),
        volume
    );
}

#[test]
fn a_devices_messages_open_on_the_pools_view_and_back_closes_them_first() {
    use zbm_display::keys::{Key, KeyPress};
    let mut model = pools();
    model.map = crate::menu::keymap::Map::Workspace;
    let log = crate::menu::world::DeviceLog {
        device: zbm_types::Untrusted::new("/dev/sdb2".to_owned()),
        ..crate::menu::world::DeviceLog::default()
    };

    events::apply(&mut model, Event::DeviceLogLoaded(log.clone()));
    assert_eq!(model.device_log, Some(log));
    let escape = KeyPress::new(Key::Escape);
    assert_eq!(super::key_press(&mut model, escape), []);
    assert_eq!(
        (model.device_log.is_none(), model.screen),
        (true, Screen::Pools)
    );
    let _ = super::key_press(&mut model, escape);
    assert_eq!(model.screen, Screen::Main);
}

#[test]
fn a_devices_messages_close_when_the_tab_is_left_or_the_machine_changes() {
    use super::Message;
    use zbm_display::keys::{Key, KeyPress};
    let mut model = pools();
    model.map = crate::menu::keymap::Map::Workspace;
    let log = crate::menu::world::DeviceLog {
        device: zbm_types::Untrusted::new("/dev/sdb2".to_owned()),
        ..crate::menu::world::DeviceLog::default()
    };
    let loaded = |model: Model| {
        let event = Message::Event(Box::new(Event::DeviceLogLoaded(log.clone())));
        super::update(model, event).0
    };
    let key =
        |model: Model, c: char| super::update(model, Message::Key(KeyPress::new(Key::Char(c)))).0;

    // To the Boot tab and back: the messages do not come back.
    let mut model = key(key(loaded(model), '1'), '3');
    assert_eq!(
        (model.screen, model.device_log.as_ref()),
        (Screen::Pools, None)
    );
    // Read once the tab is left: not shown.
    model.screen = Screen::Main;
    let mut model = loaded(model);
    assert_eq!(model.device_log, None);
    // The machine changed, as when the pool is exported.
    model.screen = Screen::Pools;
    let model = loaded(model);
    let snapshot = model.snapshot.clone();
    let model = super::update(model, Message::Event(Box::new(Event::Changed(snapshot)))).0;
    assert_eq!(model.device_log, None);
}
