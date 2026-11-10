//! The command line's keys and costs (ADR 0007, point 5; ADR 0010): each
//! entry shows the keys that do it in the view under the line and what it
//! asks for first, from what the keys do ([`action_of`]), which the keys
//! themselves follow. On the demo machine, with both maps.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};
use zbm_types::ImportPolicy;

use super::action_of::action_of;
use super::offers_tests::{Place, machine_all, menu, places, presses};
use super::{costs_of, handle, key_press, palette_items, screens};
use crate::menu::keymap::{self, Context, Input, Intent, Map};
use crate::menu::model::{Costs, Model, PaletteItem, Screen, Then};
use crate::menu::world::{Access, Action, Encryption, ImportOptions, Snapshot};
use crate::text::text;

/// The intents the keys of `context` ask for with `map`, once each.
fn intents(map: Map, context: Context) -> Vec<Intent> {
    let mut intents = Vec::new();
    for press in presses(map, context) {
        if let Some(Input::Intent(intent)) = keymap::input(map, context, press)
            && !intents.contains(&intent)
        {
            intents.push(intent);
        }
    }
    intents
}

/// The command line's entries over `model`.
fn entries_of(model: &Model) -> Vec<PaletteItem> {
    palette_items(model)
}

/// The entry of `items` that does `then`.
fn entry<'i>(items: &'i [PaletteItem], then: &Then) -> &'i PaletteItem {
    let found = items.iter().find(|item| item.then == *then);
    found.unwrap_or_else(|| panic!("no entry does {then:?}"))
}

/// What a key or an entry did: the actions sent, the dialog, the screen,
/// the command line edited and what the status line says.
fn did(model: &Model, sent: &[Action]) -> String {
    format!(
        "{sent:?} {:?} {:?} {:?} {:?}",
        model.dialog, model.screen, model.editor, model.notification
    )
}

#[test]
fn a_key_and_its_entry_send_the_same_action() {
    let machine = machine_all();
    let mut followed = 0;
    for map in [Map::Classic, Map::Workspace] {
        for place in places(&machine, map) {
            let model = menu(&machine, map, place);
            let context = model.context();
            for intent in intents(map, context) {
                let Some(then) = action_of(&model, context, intent) else {
                    continue;
                };
                if !keymap::is_offered(context, intent, &model) {
                    continue;
                }
                let at = format!("{map:?} {place:?} {intent:?}");
                let mut by_key = menu(&machine, map, place);
                let sent = handle(&mut by_key, Input::Intent(intent));
                let mut by_entry = menu(&machine, map, place);
                let then_sent = screens::send(&mut by_entry, then);
                assert_eq!(did(&by_key, &sent), did(&by_entry, &then_sent), "{at}");
                followed += 1;
            }
        }
    }
    assert!(followed > 50, "{followed}");
}

#[test]
fn each_entry_shows_the_keys_of_its_action_in_the_view_under_it() {
    let machine = machine_all();
    let model = menu(&machine, Map::Workspace, Place::Main);
    let items = entries_of(&model);
    let selected = model.selected_environment().unwrap().clone();
    // `Enter` boots the selected environment, `d` makes it the default;
    // another's entries have no keys.
    let boot = Then::Act(screens::boot(&model, &selected, None));
    let enter = text!("key-enter");
    let keys = &entry(&items, &boot).keys;
    assert!(keys.split(", ").any(|key| *key == enter), "{keys}");
    let default = Then::Act(Action::SetDefaultEnvironment(selected.name.clone()));
    assert_eq!(entry(&items, &default).keys, "d");
    let other = (model.snapshot.environments.iter()).find(|e| e.name != selected.name);
    let other = Then::Act(Action::SetDefaultEnvironment(other.unwrap().name.clone()));
    assert_eq!(entry(&items, &other).keys, "");
    // What only a character reaches has its keys as the help names them.
    assert_eq!(entry(&items, &Then::Intent(Intent::Filter)).keys, "/");

    // In the kernels, `e` edits for the selected kernel: the entry of the
    // key has it, not the edit of the environment's own command line.
    let mut model = menu(&machine, Map::Workspace, Place::Kernels(1));
    let items = entries_of(&model);
    let edit = entry(&items, &Then::Intent(Intent::Edit));
    assert!(edit.keys.starts_with('e'), "{edit:?}");
    let focus = Then::Focus(selected.name.clone(), Screen::CommandLine);
    assert_eq!(entry(&items, &focus).keys, "");
    let kernel = model.focused().unwrap().kernels[1].version.clone();
    let _ = handle(&mut model, Input::Intent(Intent::Edit));
    assert_eq!(model.editor.unwrap().kernel, Some(kernel));

    // Over every view, an entry's keys do what the entry does there.
    for map in [Map::Classic, Map::Workspace] {
        for place in places(&machine, map) {
            let model = menu(&machine, map, place);
            let context = model.view_context();
            let keyed = entries_of(&model)
                .into_iter()
                .filter(|item| !item.keys.is_empty());
            for item in keyed.filter(|item| !matches!(item.then, Then::Intent(_))) {
                let does = intents(map, context)
                    .into_iter()
                    .any(|intent| action_of(&model, context, intent) == Some(item.then.clone()));
                assert!(does, "{map:?} {place:?} {item:?}");
            }
        }
    }
}

/// The machine of `model` with every pool imported `writable`, every
/// environment's encryption `encryption` and the menu `locked`.
fn varied(snapshot: &Snapshot, writable: bool, encryption: Encryption, locked: bool) -> Snapshot {
    let mut snapshot = snapshot.clone();
    for pool in &mut snapshot.pools {
        pool.writable = writable;
    }
    for environment in &mut snapshot.environments {
        environment.encryption = encryption;
    }
    snapshot.menu_locked = locked;
    snapshot
}

/// What the pool `action` writes is, on `snapshot`: imported read-only.
fn writes_read_only(snapshot: &Snapshot, action: &Action) -> bool {
    action.needs_writable_pool()
        && action.changed_environment().is_some_and(|environment| {
            (snapshot.pools.iter())
                .any(|pool| pool.imported && pool.name == environment.pool() && !pool.writable)
        })
}

/// The actions of the command line's entries over `model`, and every
/// import's options of every pool.
fn actions_of(model: &Model) -> Vec<Action> {
    let mut actions: Vec<Action> = (entries_of(model).into_iter())
        .filter_map(|item| match item.then {
            Then::Act(action) => Some(action),
            _ => None,
        })
        .collect();
    for pool in &model.snapshot.pools {
        for access in [Access::ReadOnly, Access::ReadWrite] {
            for policy in [
                ImportPolicy::Strict,
                ImportPolicy::Hostid,
                ImportPolicy::Force,
            ] {
                for rewind in [false, true] {
                    let options = ImportOptions {
                        access,
                        policy,
                        rewind,
                        id: pool.id,
                    };
                    actions.push(Action::Import(pool.name.clone(), options));
                }
            }
        }
    }
    actions
}

/// Checks the costs of `action` on the machine of `model`, whose menu is
/// `locked` and whose environments' encryption is `encryption`; and how
/// many of the PIN, the read/write import and the key it costs.
fn check_costs(model: &Model, action: &Action, encryption: Encryption) -> [usize; 3] {
    let snapshot = &model.snapshot;
    let costs = costs_of(model, &Then::Act(action.clone()));
    let kept = action.is_kept_by_lock(snapshot.bootfs.as_ref()) || *action == Action::Shell;
    let at = format!("{encryption:?} {action:?}");
    assert_eq!(costs.pin, snapshot.menu_locked && kept, "{at}");
    assert_eq!(costs.writes, writes_read_only(snapshot, action), "{at}");
    // A boot and a shell in an environment load its key too.
    let has_key = matches!(action, Action::Boot { .. } | Action::Chroot(_))
        || action.changed_environment().is_some();
    let changes_locked = has_key && encryption == Encryption::Locked;
    assert_eq!(costs.key, changes_locked, "{at}");
    assert!(!costs.review, "{at}");
    // Only a read-only import as this host, not rewound, is free.
    if let Action::Import(_, options) = action {
        let is_free = options.access == Access::ReadOnly
            && options.policy == ImportPolicy::Strict
            && !options.rewind;
        assert_eq!(costs.pin, snapshot.menu_locked && !is_free, "{at}");
    }
    [costs.pin, costs.writes, costs.key].map(usize::from)
}

#[test]
fn the_cost_of_every_action_agrees_with_the_lock_and_the_pool_it_writes() {
    let machine = machine_all();
    let mut counts = [0; 3];
    for map in [Map::Classic, Map::Workspace] {
        for place in places(&machine, map) {
            let mut model = menu(&machine, map, place);
            let original = model.snapshot.clone();
            let actions = actions_of(&model);
            for writable in [false, true] {
                for encryption in [Encryption::Unlocked, Encryption::Locked] {
                    for locked in [false, true] {
                        model.snapshot = varied(&original, writable, encryption, locked);
                        for action in &actions {
                            let counted = check_costs(&model, action, encryption);
                            for (count, more) in counts.iter_mut().zip(counted) {
                                *count += more;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(counts.iter().all(|count| *count > 0), "{counts:?}");
}

#[test]
fn a_key_that_opens_a_sheet_carries_no_cost() {
    let mut machine = machine_all();
    machine.0.menu_locked = true;
    for map in [Map::Classic, Map::Workspace] {
        // A snapshot's actions, a pool's imports and details, and power,
        // which the lock keeps none of.
        let sheets = [
            (Place::Snapshots, Intent::Primary, true),
            (Place::Pools(0), Intent::Import, true),
            (Place::Pools(0), Intent::Primary, true),
            (Place::Main, Intent::Power, false),
        ];
        for (place, intent, has_kept) in sheets {
            let model = menu(&machine, map, place);
            let context = model.view_context();
            assert_eq!(
                action_of(&model, context, intent),
                None,
                "{place:?} {intent:?}"
            );
            assert!(
                !super::is_kept(&model, context, intent),
                "{place:?} {intent:?}"
            );
            let costs = costs_of(&model, &Then::Intent(intent));
            assert_eq!(costs, Costs::default(), "{place:?} {intent:?}");
            // The sheet's entries have theirs.
            let mut opened = menu(&machine, map, place);
            let _ = handle(&mut opened, Input::Intent(intent));
            let Some(crate::menu::model::Dialog::Choose { choices, .. }) = &opened.dialog else {
                panic!("{map:?} {place:?} {intent:?}: no sheet");
            };
            let kept = choices.iter().any(|c| costs_of(&opened, &c.then).pin);
            assert_eq!(kept, has_kept, "{map:?} {place:?} {intent:?}");
        }
    }
    // A key that sends what the lock keeps costs the PIN.
    let model = menu(&machine, Map::Workspace, Place::Snapshots);
    let costs = costs_of(&model, &Then::Intent(Intent::Rollback));
    assert!(costs.pin && costs.review, "{costs:?}");
}

#[test]
fn costs_are_not_matched() {
    let mut machine = machine_all();
    machine.0.menu_locked = true;
    let mut model = menu(&machine, Map::Workspace, Place::Main);
    let _ = key_press(&mut model, KeyPress::new(Key::Char(':')));
    // Every entry costs everything and has a key no label says.
    for item in &mut model.palette.as_mut().unwrap().items {
        item.keys = "qzx".to_owned();
        item.costs = Costs {
            pin: true,
            writes: true,
            key: true,
            review: true,
        };
    }
    for c in "qzx".chars() {
        let _ = key_press(&mut model, KeyPress::new(Key::Char(c)));
    }
    assert_eq!(model.palette.as_ref().unwrap().matches.len(), 0);
    // The cost words find what they find without them.
    let pin = keymap::lock_mark();
    let found = |model: &mut Model| {
        for c in pin.chars() {
            let _ = key_press(model, KeyPress::new(Key::Char(c)));
        }
        let palette = model.palette.take().unwrap();
        let found: Vec<String> = (palette.matches.iter())
            .map(|r| palette.items[r.index].label.clone())
            .collect();
        found
    };
    let mut open = menu(&machine_all(), Map::Workspace, Place::Main);
    let _ = key_press(&mut open, KeyPress::new(Key::Char(':')));
    let mut locked = menu(&machine, Map::Workspace, Place::Main);
    let _ = key_press(&mut locked, KeyPress::new(Key::Char(':')));
    let kept = locked
        .palette
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter(|i| i.costs.pin);
    assert!(kept.count() > 0);
    assert_eq!(found(&mut locked), found(&mut open));
}

/// The keys bound in `context` with `map` whose intent the toolbar or help
/// names there, each with what it does: the toolbar's and those only help
/// names, as `Delete` and the workspace's `b`.
fn bound(model: &Model, map: Map, context: Context) -> Vec<(String, Then)> {
    let listed = keymap::commands(map, context);
    let table = map.table().into_iter();
    table
        .filter(|binding| map.is_bound(binding, context))
        .filter(|binding| keymap::is_shown(context, binding.intent, model))
        .filter(|binding| !listed.iter().any(|(intent, _)| *intent == binding.intent))
        .filter_map(|binding| {
            let then = action_of(model, context, binding.intent)?;
            Some((keymap::key_name(binding.key, binding.modifiers), then))
        })
        .collect()
}

#[test]
fn every_key_that_does_an_entry_s_action_is_shown_on_it() {
    let machine = machine_all();
    let mut checked = 0;
    for map in [Map::Classic, Map::Workspace] {
        for place in places(&machine, map) {
            let model = menu(&machine, map, place);
            let context = model.view_context();
            let items = entries_of(&model);
            for (key, then) in bound(&model, map, context) {
                let Some(item) = items.iter().find(|item| item.then == then) else {
                    continue;
                };
                let at = format!("{map:?} {place:?} {key} {item:?}");
                assert!(item.keys.split(", ").any(|shown| shown == key), "{at}");
                checked += 1;
            }
        }
    }
    assert!(checked > 20, "{checked}");
    // Keys the toolbar leaves out: the workspace's `b`, which boots the
    // selection, and `Delete`, which destroys the selected snapshot.
    let model = menu(&machine, Map::Workspace, Place::Main);
    let selected = model.selected_environment().unwrap();
    let boot = Then::Act(screens::boot(&model, selected, None));
    let enter = text!("key-enter");
    assert_eq!(
        entry(&entries_of(&model), &boot).keys,
        format!("{enter}, b")
    );
    let model = menu(&machine, Map::Workspace, Place::Snapshots);
    let items = entries_of(&model);
    let snapshot = model.selected_snapshot().unwrap().name.clone();
    let clone = entry(&items, &Then::BootClone(snapshot.clone()));
    assert_eq!(clone.keys, "b");
    let destroy = action_of(&model, Context::Snapshots, Intent::Destroy).unwrap();
    let keys = &entry(&items, &destroy).keys;
    assert!(
        keys.split(", ").any(|key| key == text!("key-delete")),
        "{keys}"
    );
}

#[test]
fn a_promotion_costs_its_review_as_it_asks_for_a_typed_name() {
    let machine = machine_all();
    let model = menu(&machine, Map::Workspace, Place::Snapshots);
    let context = Context::Snapshots;
    let promote = action_of(&model, context, Intent::CloneAndPromote).unwrap();
    assert!(costs_of(&model, &promote).review, "{promote:?}");
    let clone = action_of(&model, context, Intent::Clone).unwrap();
    assert!(!costs_of(&model, &clone).review, "{clone:?}");
    // Once named, the promotion asks for the environment's name.
    let mut named = menu(&machine, Map::Workspace, Place::Snapshots);
    let _ = handle(&mut named, Input::Intent(Intent::CloneAndPromote));
    let _ = handle(&mut named, Input::Intent(Intent::Primary));
    assert!(
        matches!(
            named.dialog,
            Some(crate::menu::model::Dialog::Confirm { .. })
        ),
        "{:?}",
        named.dialog
    );
}

#[test]
fn booting_or_opening_a_shell_in_a_locked_environment_costs_its_key() {
    let machine = machine_all();
    let mut model = menu(&machine, Map::Workspace, Place::Main);
    let environment = model.selected_environment().unwrap().clone();
    let boot = Then::Act(screens::boot(&model, &environment, None));
    let chroot = Then::Act(Action::Chroot(environment.name.clone().into()));
    assert!(!costs_of(&model, &boot).key);
    model.snapshot = varied(&model.snapshot, true, Encryption::Locked, false);
    assert!(costs_of(&model, &boot).key);
    assert!(costs_of(&model, &chroot).key);
    let items = entries_of(&model);
    assert!(entry(&items, &boot).costs.key);
}
