//! The ways the menu imports a pool (ADR 0016, point 3): one that is
//! imported read-only or read/write again, or rewound to its checkpoint;
//! one that is not as this host, as the host that owns it, or forced.

use pretty_assertions::assert_eq;
use zbm_types::{ImportPolicy, PoolName};

use super::{dialogs, events, screens};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine};
use crate::menu::keymap::Intent;
use crate::menu::model::{Choice, Dialog, Model, Screen, Then};
use crate::menu::theme::Theme;
use crate::menu::world::{Access, Action, ImportOptions, Offer, Outcome};

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
}

#[test]
fn the_details_of_a_pool_offer_its_imports() {
    let tank: PoolName = "tank".parse().unwrap();
    let mut model = pools();
    dialogs::pool_details(&mut model, &tank);
    assert_eq!(
        choices(&model),
        [
            import("tank", Access::ReadOnly, ImportPolicy::Strict, false),
            import("tank", Access::ReadWrite, ImportPolicy::Strict, true),
        ]
    );

    let backup: PoolName = "backup".parse().unwrap();
    dialogs::pool_details(&mut model, &backup);
    assert_eq!(
        choices(&model),
        [
            import("backup", Access::ReadOnly, ImportPolicy::Hostid, false),
            import("backup", Access::ReadOnly, ImportPolicy::Force, false),
        ]
    );
}
