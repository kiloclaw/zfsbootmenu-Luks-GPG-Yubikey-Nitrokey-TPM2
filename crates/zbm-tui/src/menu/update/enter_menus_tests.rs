//! Rule 1 of decision 18 (ADR 0007), as the maintainer amended it for the
//! workspace: each action a character reaches is an entry of the `Enter`
//! menu of what it applies to, a snapshot, a pool or a volume, and does
//! there what the key does; where `Enter` boots, an environment's and a
//! kernel's other actions are entries of the command line, and what
//! applies to a view is a command. On the demo machine; the classic map
//! keeps the palette's reach (`commands_tests.rs`).

use std::collections::BTreeSet;

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::action_of::action_of;
use super::commands_tests::{
    Effect, Machine, chosen, effect, entered, has_other_key, intent_of, is_reached, items,
};
use super::key_press;
use super::offers_tests::{Place, machine_all, menu, places, presses};
use crate::menu::keymap::{self, Context, Intent, Map, Reach};
use crate::menu::model::{Choice, Dialog, Model, Then};
use crate::menu::world::{Action, Outcome, Pool};
use crate::text::text;

/// A key press of `key`.
fn press(key: Key) -> KeyPress {
    KeyPress::new(key)
}

/// The keys that mark the older of the demo's two snapshots, in a folded
/// group of the timeline: down to the group, open it, mark the snapshot.
fn mark_older() -> [KeyPress; 3] {
    [
        press(Key::Down),
        press(Key::Char('z')),
        press(Key::Char(' ')),
    ]
}

/// The keys that mark both of the demo's snapshots, the newer selected
/// last, which `f` then diffs.
fn mark_both() -> Vec<KeyPress> {
    let up = [press(Key::Up), press(Key::Char(' '))];
    [&mark_older()[..], &up].concat()
}

/// The marks made before a snapshot's keys: none, the selected snapshot
/// marked, and two marked; none elsewhere.
fn marks(place: Place) -> Vec<Vec<KeyPress>> {
    if place != Place::Snapshots {
        return vec![Vec::new()];
    }
    vec![Vec::new(), vec![press(Key::Char(' '))], mark_both()]
}

/// The pool of the row at `place` with `map`, if it is one of a pool's
/// rows.
fn pool_at(machine: &Machine, map: Map, place: Place) -> Option<Pool> {
    menu(machine, map, place).selected_pool().cloned()
}

/// Where `Enter` opens the menu of what the keys at `place` apply to: on
/// a vdev or a device of the pools' tree, its pool's row, which comes
/// first; elsewhere `place` itself.
fn target(machine: &Machine, place: Place) -> Place {
    let map = Map::Workspace;
    let Place::Pools(_) = place else {
        return place;
    };
    let Some(pool) = pool_at(machine, map, place) else {
        return place;
    };
    let rows = menu(machine, map, place).pools.matches.len();
    (0..rows)
        .map(Place::Pools)
        .find(|row| pool_at(machine, map, *row).as_ref() == Some(&pool))
        .unwrap_or(place)
}

/// The entries of the menu `model` shows, if it shows one.
fn choices(model: &Model) -> &[Choice] {
    match &model.dialog {
        Some(Dialog::Choose { choices, .. }) => choices,
        _ => &[],
    }
}

/// Whether what a key did is done by an entry of the `Enter` menu
/// (`entered`: `Enter`, then each entry); or the key opened a menu whose
/// every entry is one of the `Enter` menu's, as `i` does.
fn is_in_menu(key: &Effect, entered: &[Effect]) -> bool {
    if entered.contains(key) {
        return true;
    }
    let of_enter = entered.first().map_or(&[][..], |(_, model)| choices(model));
    let of_key = choices(&key.1);
    !of_key.is_empty()
        && of_key
            .iter()
            .all(|choice| of_enter.iter().any(|entry| entry.then == choice.then))
}

/// Whether the key's effect `key` is nothing but saying why, as a rewind
/// of a pool without a checkpoint says.
fn is_said_only(key: &mut Effect, unchanged: &Effect) -> bool {
    let said = key.1.notification.clone();
    key.1.notification.clone_from(&unchanged.1.notification);
    let is_said = *key == *unchanged;
    key.1.notification = said;
    is_said
}

/// What `keys` do to the item at `place`: on a vdev or a device of the
/// pools' tree, whose keys act on its pool, what they do on its pool's
/// row, which they do here too.
fn on_item(machine: &Machine, place: Place, keys: &[KeyPress]) -> Effect {
    let map = Map::Workspace;
    let item = target(machine, place);
    let there = effect(machine, map, item, keys);
    if item != place {
        let here = effect(machine, map, place, keys);
        let sent = (&here.0, &here.1.dialog);
        assert_eq!(sent, (&there.0, &there.1.dialog), "{place:?} {keys:?}");
    }
    there
}

/// Checks each key of a character at `place` of the workspace after the
/// marks `before`: what it does is an entry where [`keymap::reach`] says,
/// the `Enter` menu of what it applies to or the command line; adds each
/// place found to `reaches`, and returns how many keys it checked.
fn check_keys(
    machine: &Machine,
    place: Place,
    before: &[KeyPress],
    reaches: &mut BTreeSet<String>,
) -> usize {
    let map = Map::Workspace;
    let context = menu(machine, map, place).context();
    let palette = items(machine, map, place);
    let chosen: Vec<Effect> = (0..palette.len())
        .map(|index| chosen(machine, map, place, index))
        .collect();
    let enter = entered(machine, map, target(machine, place), before);
    let unchanged = effect(machine, map, place, before);
    let mut checked = 0;
    for key in presses(map, context) {
        let Some(intent) = intent_of(map, context, key) else {
            continue;
        };
        // The command line needs no entry of its own; a digit only
        // shortens the way to a tab, which `Tab` reaches.
        let is_character = matches!(key.key, Key::Char(_));
        let skipped = matches!(intent, Intent::Palette | Intent::Tab(_));
        if !is_character || skipped || has_other_key(map, context, intent) {
            continue;
        }
        let at = format!("{place:?} {before:?} {key:?} {intent:?}");
        let reach = keymap::reach(context, intent).expect(&at);
        let keys = [before, &[key]].concat();
        let mut by_key = effect(machine, map, place, &keys);
        if is_said_only(&mut by_key, &unchanged) {
            continue;
        }
        let found = match reach {
            Reach::Menu | Reach::MenuAndCommand => {
                is_in_menu(&on_item(machine, place, &keys), &enter)
            }
            // The command line is the same with marks, but for a diff.
            _ if !before.is_empty() => continue,
            Reach::CommandLine | Reach::Command => is_reached(&by_key, &[], &chosen, &palette),
        };
        assert!(found, "{at} {reach:?}");
        reaches.insert(format!("{reach:?}"));
        checked += 1;
    }
    checked
}

/// The demo machine with each imported pool imported read/write.
fn read_write() -> Machine {
    let (mut snapshot, event) = machine_all();
    for pool in snapshot.pools.iter_mut().filter(|pool| pool.imported) {
        pool.writable = true;
    }
    (snapshot, event)
}

#[test]
fn every_action_bound_to_a_character_has_an_entry_in_the_enter_menu_of_what_it_applies_to() {
    let mut reaches = BTreeSet::new();
    let mut checked = 0;
    // Its pools imported read-only, as on the demo machine, and read/write.
    for (machine, all) in [(machine_all(), true), (read_write(), false)] {
        for place in places(&machine, Map::Workspace) {
            // Start-up binds no character but `?` (ADR 0016, point 9).
            let pools = matches!(place, Place::Pools(_));
            if place == Place::Startup || (!all && !pools) {
                continue;
            }
            for before in marks(place) {
                checked += check_keys(&machine, place, &before, &mut reaches);
            }
        }
    }
    assert_eq!(reaches.len(), 4, "{reaches:?}");
    assert!(checked > 100, "{checked}");
}

#[test]
fn w_on_a_pool_imported_read_write_says_so_and_its_details_offer_read_only() {
    let machine = read_write();
    let map = Map::Workspace;
    let rows = menu(&machine, map, Place::Pools(0)).pools.matches.len();
    let place = (0..rows)
        .map(Place::Pools)
        .find(|place| pool_at(&machine, map, *place).is_some_and(|pool| pool.imported))
        .expect("an imported pool");
    let pool = pool_at(&machine, map, place).expect("a pool");
    let model = menu(&machine, map, place);
    assert_eq!(action_of(&model, Context::Pools, Intent::ReadWrite), None);
    let (sent, after) = effect(&machine, map, place, &[press(Key::Char('w'))]);
    assert_eq!(sent, []);
    let said = text!("pool-read-write-already", pool = pool.name.to_string());
    assert_eq!(after.notification, Some(Outcome::Warning(said)));
    let offered = details(&machine, map, place);
    let labels: Vec<&str> = offered.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels[..2],
        [text!("pool-import-read-only"), text!("pool-export")]
    );
}

/// The first row of the pools view whose pool is not imported, on the
/// demo machine with `map`.
fn not_imported(machine: &Machine, map: Map) -> Place {
    let rows = menu(machine, map, Place::Pools(0)).pools.matches.len();
    (0..rows)
        .map(Place::Pools)
        .find(|place| pool_at(machine, map, *place).is_some_and(|pool| !pool.imported))
        .expect("a pool not imported")
}

/// The details of the pool at `place`, as `Enter` opens them with `map`.
fn details(machine: &Machine, map: Map, place: Place) -> Vec<Choice> {
    let mut model = menu(machine, map, place);
    let _ = key_press(&mut model, press(Key::Enter));
    choices(&model).to_vec()
}

#[test]
fn a_pool_s_details_offer_every_import_its_keys_do_when_it_is_not_imported() {
    let machine = machine_all();
    let map = Map::Workspace;
    let place = not_imported(&machine, map);
    let offered = details(&machine, map, place);
    let labels: Vec<&str> = offered.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            text!("pool-import-this-host"),
            text!("pool-import-read-write"),
            text!("pool-import-last-host"),
            text!("pool-force-import"),
            text!("pool-rewind"),
        ]
    );
    // Each of `i`'s imports, and what `w` and `Ctrl-R` do.
    let mut model = menu(&machine, map, place);
    let _ = key_press(&mut model, press(Key::Char('i')));
    let imports = choices(&model).iter().map(|choice| choice.then.clone());
    let model = menu(&machine, map, place);
    let keys = [Intent::ReadWrite, Intent::Rewind];
    let by_keys = keys.map(|intent| action_of(&model, Context::Pools, intent));
    for then in imports.chain(by_keys.into_iter().flatten()) {
        assert!(offered.iter().any(|choice| choice.then == then), "{then:?}");
    }
    // The classic map's details are as they were.
    let place = not_imported(&machine, Map::Classic);
    let classic = details(&machine, Map::Classic, place);
    let labels: Vec<&str> = classic.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels,
        [text!("pool-import-last-host"), text!("pool-force-import")]
    );
}

/// The sheet of the selected snapshot after `before`, with `map`.
fn sheet(machine: &Machine, map: Map, before: &[KeyPress]) -> Vec<Choice> {
    let mut model = menu(machine, map, Place::Snapshots);
    for key in before.iter().chain([&press(Key::Enter)]) {
        let _ = key_press(&mut model, *key);
    }
    choices(&model).to_vec()
}

/// What `f` does after `before`, on the Snapshots tab with `map`.
fn diff_key(machine: &Machine, map: Map, before: &[KeyPress]) -> Vec<Action> {
    let mut model = menu(machine, map, Place::Snapshots);
    for key in before {
        let _ = key_press(&mut model, *key);
    }
    key_press(&mut model, press(Key::Char('f')))
}

#[test]
fn the_sheet_of_a_snapshot_diffs_the_two_marked_ones_as_f_does() {
    let machine = machine_all();
    let both = mark_both();
    let marked = text!("snapshot-diff-marked");
    let sheet_both = sheet(&machine, Map::Workspace, &both);
    let entry = sheet_both.iter().find(|choice| choice.label == marked);
    let Some(Choice {
        then: Then::Act(diff),
        ..
    }) = entry
    else {
        panic!("{sheet_both:?}");
    };
    assert!(matches!(diff, Action::Diff(_, Some(_))), "{diff:?}");
    assert_eq!(
        diff_key(&machine, Map::Workspace, &both),
        std::slice::from_ref(diff)
    );
    // After its diff to now, before its chroot.
    let labels: Vec<&str> = sheet_both.iter().map(|c| c.label.as_str()).collect();
    let at = labels.iter().position(|label| *label == marked);
    let to_now = labels.iter().position(|l| *l == text!("snapshot-diff"));
    assert_eq!(at, to_now.map(|index| index + 1));

    // None with fewer marks, nor in the classic map.
    let space = [press(Key::Char(' '))];
    for (map, before) in [
        (Map::Workspace, &[][..]),
        (Map::Workspace, &space[..]),
        (Map::Workspace, &mark_older()[..]),
        (Map::Classic, &both[..]),
    ] {
        let sheet = sheet(&machine, map, before);
        assert!(!sheet.is_empty(), "{map:?} {before:?}");
        assert!(
            sheet.iter().all(|c| c.label != marked),
            "{map:?} {before:?}"
        );
    }

    // One marked, not the selected: `f` diffs that one, as its own sheet
    // does.
    let one = [&mark_older()[..], &[press(Key::Up)]].concat();
    let own = sheet(&machine, Map::Workspace, &mark_older());
    let to_now = own.iter().find(|c| c.label == text!("snapshot-diff"));
    let by_sheet = to_now.map(|choice| choice.then.clone());
    let by_key = diff_key(&machine, Map::Workspace, &one);
    assert_eq!(by_sheet, by_key.into_iter().next().map(Then::Act));
}
