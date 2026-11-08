//! Rule 1 of decision 18 (ADR 0007): what only a character or a chord
//! reaches is in the palette of its view, and choosing it there does what
//! the key does; on the demo machine, with both maps.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::key_press;
use super::offers_tests::{APP, Place, machine, machine_all, menu, places, presses};
use crate::menu::keymap::{self, Context, ENTER_REACHES, Input, Intent, Map};
use crate::menu::model::{Dialog, Model, Then};
use crate::menu::world::{Action, Event, Offer, Snapshot};

/// The demo machine.
type Machine = (Snapshot, Event);

/// What a key or a few did: the actions sent, and the menu after, which
/// counts no keys.
type Effect = (Vec<Action>, Model);

/// What `keys` do at `place`, from the start.
fn effect(machine: &Machine, map: Map, place: Place, keys: &[KeyPress]) -> Effect {
    let mut model = menu(machine, map, place);
    let sent = keys
        .iter()
        .flat_map(|key| key_press(&mut model, *key))
        .collect();
    model.keys = 0;
    (sent, model)
}

/// What choosing the entry `index` of the palette at `place` does: `:`,
/// the entry selected, `Enter`.
fn chosen(machine: &Machine, map: Map, place: Place, index: usize) -> Effect {
    let mut model = menu(machine, map, place);
    let _ = key_press(&mut model, KeyPress::new(Key::Char(':')));
    let palette = model.palette.as_mut().expect("the palette opens");
    palette.selected = palette
        .matches
        .iter()
        .position(|r| r.index == index)
        .unwrap();
    let sent = key_press(&mut model, KeyPress::new(Key::Enter));
    model.keys = 0;
    (sent, model)
}

/// The entries of the palette at `place`.
fn entries(machine: &Machine, map: Map, place: Place) -> Vec<Then> {
    let mut model = menu(machine, map, place);
    let _ = key_press(&mut model, KeyPress::new(Key::Char(':')));
    let palette = model.palette.expect("the palette opens");
    palette.items.into_iter().map(|item| item.then).collect()
}

/// What `Enter` at `place` does after the keys `before`, and each entry of
/// the menu it opens.
fn entered(machine: &Machine, map: Map, place: Place, before: &[KeyPress]) -> Vec<Effect> {
    let enter = KeyPress::new(Key::Enter);
    let first = effect(machine, map, place, &[before, &[enter]].concat());
    let count = match &first.1.dialog {
        Some(Dialog::Choose { choices, .. }) => choices.len(),
        _ => 0,
    };
    let mut effects = vec![first];
    for index in 0..count {
        let mut keys = [before, &[enter]].concat();
        keys.extend(std::iter::repeat_n(KeyPress::new(Key::Down), index));
        keys.push(enter);
        effects.push(effect(machine, map, place, &keys));
    }
    effects
}

/// Whether `intent` has a key in `context` that is no character: `Enter`,
/// an arrow, `Tab`, `Esc`, `Delete`, a page or a function key.
fn has_other_key(map: Map, context: Context, intent: Intent) -> bool {
    map.table().into_iter().any(|binding| {
        binding.intent == intent
            && map.is_bound(binding, context)
            && !matches!(binding.key, Key::Char(_))
    })
}

/// The intent `press` asks for in `context`, if it is one.
fn intent_of(map: Map, context: Context, press: KeyPress) -> Option<Intent> {
    match keymap::input(map, context, press) {
        Some(Input::Intent(intent)) => Some(intent),
        _ => None,
    }
}

/// Whether what `key` did is done by an entry of the palette, by `Enter`
/// or an entry of its menu (`others`); or the key opened a menu whose
/// every entry the palette has, as power's.
fn is_reached(key: &Effect, others: &[Effect], palette: &[Then]) -> bool {
    if others.iter().any(|other| other == key) {
        return true;
    }
    match &key.1.dialog {
        Some(Dialog::Choose { choices, .. }) => {
            choices.iter().all(|choice| palette.contains(&choice.then))
        }
        _ => false,
    }
}

#[test]
fn every_action_only_a_character_reaches_is_in_the_palette_of_its_view() {
    let machine = machine_all();
    let mut checked = 0;
    for map in [Map::Classic, Map::Workspace] {
        // Start-up binds no character but `?` (ADR 0016, point 9).
        for place in places(&machine, map)
            .into_iter()
            .filter(|p| *p != Place::Startup)
        {
            let context = menu(&machine, map, place).context();
            let palette = entries(&machine, map, place);
            let mut others: Vec<Effect> = (0..palette.len())
                .map(|index| chosen(&machine, map, place, index))
                .collect();
            others.extend(entered(&machine, map, place, &[]));
            let unchanged = effect(&machine, map, place, &[]);
            for press in presses(map, context) {
                let Some(intent) = intent_of(map, context, press) else {
                    continue;
                };
                // The palette needs no entry of its own; a digit only
                // shortens the way to a tab (`keymap/commands_tests.rs`).
                let is_character = matches!(press.key, Key::Char(_));
                let skipped = matches!(intent, Intent::Palette | Intent::Tab(_));
                if !is_character || skipped || has_other_key(map, context, intent) {
                    continue;
                }
                let key = effect(&machine, map, place, &[press]);
                if key == unchanged {
                    continue;
                }
                let at = format!("{map:?} {place:?} {press:?} {intent:?}");
                assert!(is_reached(&key, &others, &palette), "{at}");
                checked += 1;
            }
        }
    }
    assert!(checked > 40, "{checked}");
}

/// The palette's entry at `place` of a key's `intent`, as its index.
fn entry_of(machine: &Machine, map: Map, place: Place, intent: Intent) -> Option<usize> {
    let palette = entries(machine, map, place);
    palette
        .iter()
        .position(|then| *then == Then::Intent(intent))
}

#[test]
fn choosing_a_key_s_entry_does_what_the_key_does() {
    let machine = machine_all();
    let key = |c| KeyPress::new(Key::Char(c));
    let cases = [
        (Map::Workspace, Place::Snapshots, key('o'), Intent::Sort),
        (Map::Classic, Place::Snapshots, key('o'), Intent::Sort),
        (Map::Workspace, Place::Snapshots, key('z'), Intent::Fold),
        (Map::Workspace, Place::Snapshots, key(' '), Intent::Mark),
        (Map::Workspace, Place::Main, key('d'), Intent::SetDefault),
        (
            Map::Classic,
            Place::Kernels(1),
            key('d'),
            Intent::SetDefault,
        ),
        (Map::Workspace, Place::Main, key('n'), Intent::NewSnapshot),
        (
            Map::Classic,
            Place::Snapshots,
            key('n'),
            Intent::NewSnapshot,
        ),
    ];
    for (map, place, press, intent) in cases {
        let at = format!("{map:?} {place:?} {intent:?}");
        let index = entry_of(&machine, map, place, intent).expect(&at);
        let by_key = effect(&machine, map, place, &[press]);
        let by_entry = chosen(&machine, map, place, index);

        assert_eq!(by_entry, by_key, "{at}");
        assert_ne!(by_key, effect(&machine, map, place, &[]), "{at}");
        assert_eq!(by_entry.1.palette, None, "{at}");
    }
    // The mark is on the selected snapshot.
    let (_, model) = chosen(
        &machine,
        Map::Workspace,
        Place::Snapshots,
        entry_of(&machine, Map::Workspace, Place::Snapshots, Intent::Mark).unwrap(),
    );
    let selected = model.selected_snapshot().map(|s| s.name.clone());
    assert_eq!(model.marked.iter().next().cloned(), selected);
}

#[test]
fn an_entry_of_what_the_machine_does_not_offer_is_not_listed() {
    let main = [Intent::SetDefault, Intent::NewSnapshot, Intent::ReadWrite];
    for map in [Map::Classic, Map::Workspace] {
        let all = machine_all();
        let boot = machine(&[Offer::Boot]);
        for intent in main {
            assert!(
                entry_of(&all, map, Place::Main, intent).is_some(),
                "{intent:?}"
            );
            assert_eq!(
                entry_of(&boot, map, Place::Main, intent),
                None,
                "{intent:?}"
            );
        }
        // What starts nothing on the machine is always listed.
        assert!(entry_of(&boot, map, Place::Main, Intent::Filter).is_some());
        let app = machine(APP);
        assert!(entry_of(&app, map, Place::Snapshots, Intent::Sort).is_some());
    }
}

#[test]
fn choosing_a_key_s_entry_always_does_something() {
    let machine = machine_all();
    let mut checked = 0;
    for map in [Map::Classic, Map::Workspace] {
        // Start-up has no palette.
        for place in places(&machine, map)
            .into_iter()
            .filter(|p| *p != Place::Startup)
        {
            // The classic map shows its pools' keys on a volume, which
            // they do nothing to, in its toolbar too (ADR 0016, point 10).
            let model = menu(&machine, map, place);
            if map == Map::Classic && model.selected_volume().is_some() {
                continue;
            }
            let unchanged = effect(&machine, map, place, &[]);
            for (index, then) in entries(&machine, map, place).iter().enumerate() {
                if matches!(then, Then::Intent(_)) {
                    let at = format!("{map:?} {place:?} {then:?}");
                    assert_ne!(chosen(&machine, map, place, index), unchanged, "{at}");
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 40, "{checked}");
}

#[test]
fn a_key_the_selected_row_does_not_take_has_no_entry() {
    let machine = machine_all();
    let map = Map::Workspace;
    // A volume's row takes none of its pool's keys, which a pool's row
    // of the same tree lists.
    let volume = enter_place(&machine, map, Context::Pools);
    let rows = menu(&machine, map, Place::Pools(0)).pools.matches.len();
    for intent in [Intent::Import, Intent::ReadWrite, Intent::Rewind] {
        assert_eq!(entry_of(&machine, map, volume, intent), None, "{intent:?}");
        let listed =
            (0..rows).any(|row| entry_of(&machine, map, Place::Pools(row), intent).is_some());
        assert!(listed, "{intent:?}");
    }
    // The header of a folded group marks nothing.
    assert!(entry_of(&machine, map, Place::Snapshots, Intent::Mark).is_some());
    let mut model = menu(&machine, map, Place::Snapshots);
    for c in ['z', ':'] {
        let _ = key_press(&mut model, KeyPress::new(Key::Char(c)));
    }
    assert!(model.folded_group().is_some());
    let palette = model.palette.expect("the palette opens");
    assert!(
        !palette
            .items
            .iter()
            .any(|item| item.then == Then::Intent(Intent::Mark))
    );
}

/// A place of `context` where `Enter` opens what [`ENTER_REACHES`] says,
/// with `map`: a snapshot, or a volume of the pools.
fn enter_place(machine: &Machine, map: Map, context: Context) -> Place {
    if context == Context::Snapshots {
        return Place::Snapshots;
    }
    let rows = menu(machine, map, Place::Pools(0)).pools.matches.len();
    (0..rows)
        .map(Place::Pools)
        .find(|place| menu(machine, map, *place).selected_volume().is_some())
        .expect("a volume")
}

#[test]
fn what_an_enter_menu_reaches_is_in_that_menu() {
    let machine = machine_all();
    let press = |key| KeyPress::new(key);
    // Another snapshot marked, which a snapshot's key may act on instead.
    let marked = [press(Key::Down), press(Key::Char(' ')), press(Key::Up)];
    let mut checked = 0;
    for map in [Map::Classic, Map::Workspace] {
        for (context, intents) in ENTER_REACHES {
            let place = enter_place(&machine, map, *context);
            let mut befores = vec![Vec::new()];
            if *context == Context::Snapshots {
                befores.push(marked.to_vec());
            }
            for before in befores {
                let menu = entered(&machine, map, place, &before);
                for key in presses(map, *context) {
                    let Some(intent) = intent_of(map, *context, key) else {
                        continue;
                    };
                    if !intents.contains(&intent) {
                        continue;
                    }
                    let by_key =
                        effect(&machine, map, place, &[before.as_slice(), &[key]].concat());
                    assert!(
                        menu.contains(&by_key),
                        "{map:?} {context:?} {before:?} {key:?}"
                    );
                    checked += 1;
                }
            }
        }
    }
    // Each intent of the list, in each map, by its letter or its alias.
    assert!(checked >= 12, "{checked}");
}
