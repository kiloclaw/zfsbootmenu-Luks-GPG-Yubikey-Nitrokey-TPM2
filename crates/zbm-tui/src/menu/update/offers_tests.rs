//! The menu sends only what the machine offers (ADR 0016, point 2): every
//! key of every view, every entry of the `Enter` menus, of the recovery
//! tools and of the palette, over machines that offer chosen kinds.

use std::collections::BTreeSet;

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::{events, key_press, palette_items, screens};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine};
use crate::menu::keymap::{self, CLASSIC, Context, Map, Modifiers};
use crate::menu::model::{Dialog, Model, PaletteItem, Screen, Then};
use crate::menu::theme::Theme;
use crate::menu::world::{Access, Action, Event, Offer, Outcome, Snapshot, StageState};
use crate::text::text;
use zbm_types::ImportPolicy;

/// What the application offers today.
pub(super) const APP: &[Offer] = &[
    Offer::Boot,
    Offer::BootSnapshot,
    Offer::SetDefaultEnvironment,
    Offer::DefaultKernel,
    Offer::CommandLine,
    Offer::CreateSnapshot,
    Offer::Rollback,
    Offer::Destroy,
    Offer::Clone,
    Offer::Duplicate,
    Offer::Diff,
    Offer::RestoreFiles,
    Offer::Import,
    Offer::Export,
    Offer::Shell,
    Offer::FirmwareSetup,
    Offer::Power,
];

/// Where keys are pressed: a view, with a row selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Place {
    Startup,
    Main,
    Kernels(usize),
    Snapshots,
    Pools(usize),
    Recovery(usize),
    Files,
    Packages,
    Log,
}

/// The demo machine with every stage done, offering `offers`: a pool of
/// another host, a TPM2 key to seal again, boot tries used up.
pub(super) fn machine(offers: &[Offer]) -> (Snapshot, Event) {
    let options = DemoOptions {
        foreign: true,
        snapshots: 3,
        failing: true,
        tpm_changed: true,
        ..DemoOptions::default()
    };
    let mut demo = FakeMachine::new(&options);
    // The snapshots of the environment the menu selects: the fallback of
    // the default, which used up its tries.
    let selected = demo
        .snapshot()
        .fallback
        .as_ref()
        .and_then(|f| f.target.clone());
    demo.apply(Action::LoadSnapshots(selected.unwrap()));
    let mut snapshot = demo.snapshot().clone();
    for stage in &mut snapshot.startup {
        stage.state = StageState::Done;
    }
    snapshot.offers = offers.iter().copied().collect();
    let from = snapshot.loaded[0].snapshots[0].name.clone();
    let diff = demo.apply(Action::Diff(from, None)).remove(0);
    (snapshot, diff)
}

/// The menu on `machine` with the key map `map`, at `place`.
pub(super) fn menu((snapshot, diff): &(Snapshot, Event), map: Map, place: Place) -> Model {
    let mut model = Model::new(snapshot.clone(), Theme::new(Capabilities::FULL));
    model.map = map;
    // The workspace's Pools tab is its tree.
    model.refresh();
    if place == Place::Startup {
        return model;
    }
    events::start(&mut model);
    model.countdown = None;
    let environment = model.selected_environment().map(|e| e.name.clone());
    match (place, environment) {
        (Place::Kernels(row), Some(name)) => {
            screens::focus(&mut model, name, Screen::Kernels);
            model.kernels.selected = row;
        }
        // Its list read, as the machine answers the load the focus asks
        // for: the marks then know the snapshots' ages.
        (Place::Snapshots, Some(name)) => {
            let list = snapshot.loaded.iter().find(|list| list.environment == name);
            let list = list.cloned();
            screens::focus(&mut model, name, Screen::Snapshots);
            if let Some(list) = list {
                events::apply(&mut model, Event::SnapshotsLoaded(list));
            }
        }
        (Place::Pools(row), _) => {
            model.screen = Screen::Pools;
            model.pools.selected = row;
        }
        (Place::Recovery(row), _) => {
            model.screen = Screen::Recovery;
            model.recovery.selected = row;
        }
        (Place::Files | Place::Packages, _) => {
            events::apply(&mut model, diff.clone());
            if let (Place::Packages, Some(view)) = (place, &mut model.diff) {
                view.switch();
            }
        }
        (Place::Log, _) => model.screen = Screen::Log,
        _ => {}
    }
    model
}

/// What `then` asks for, once named, confirmed or reviewed.
fn then_action(model: &Model, then: &Then) -> Option<Action> {
    match then {
        Then::Act(action) | Then::Confirm { action, .. } | Then::Review(action) => {
            Some(action.clone())
        }
        Then::Name(naming) => naming.action(),
        Then::BootClone(snapshot) => {
            then_action(model, &super::snapshots::boot_clone(model, snapshot))
        }
        Then::Focus(name, Screen::CommandLine) => {
            let environment = model
                .snapshot
                .environments
                .iter()
                .find(|e| &e.name == name)?;
            Some(screens::boot(model, environment, None))
        }
        Then::EditFor(name, kernel) => {
            let environment = model
                .snapshot
                .environments
                .iter()
                .find(|e| &e.name == name)?;
            Some(screens::boot(model, environment, Some(kernel.clone())))
        }
        Then::SaveCommandLine(name) => {
            let environment = model
                .snapshot
                .environments
                .iter()
                .find(|e| &e.name == name)?;
            let line = model.command_line(environment);
            Some(Action::SetCommandLine(name.clone(), line))
        }
        // A key's, which the keys of the view reach too.
        Then::Open(_) | Then::Focus(..) | Then::Intent(_) | Then::Go(_) | Then::Revert(_) => None,
    }
}

/// The actions `sent`, and those what is now open can end in: the entries
/// of a menu or of the palette, the action a confirmation, a review page
/// or a name dialog asks for, the boot of the command line editor.
fn reached(model: &Model, mut sent: Vec<Action>) -> Vec<Action> {
    match &model.dialog {
        Some(Dialog::Choose { choices, .. }) => {
            sent.extend(choices.iter().filter_map(|c| then_action(model, &c.then)));
        }
        Some(Dialog::Confirm { action, .. }) => sent.push(action.clone()),
        Some(Dialog::Review(reviewing)) => sent.push(reviewing.action.clone()),
        Some(Dialog::Name(naming)) => sent.extend(naming.action()),
        _ => {}
    }
    if model.palette.is_some() {
        let items = palette_items(model);
        sent.extend(
            items
                .iter()
                .filter_map(|item| then_action(model, &item.then)),
        );
    }
    if let Some(editor) = &model.editor {
        let then = if editor.save {
            Then::SaveCommandLine(editor.environment.clone())
        } else {
            Then::Focus(editor.environment.clone(), Screen::CommandLine)
        };
        sent.extend(then_action(model, &then));
    }
    sent
}

/// Every key bound in `context` with `map`: the map's and the classic
/// ones.
pub(super) fn presses(map: Map, context: Context) -> Vec<KeyPress> {
    let bound = map
        .table()
        .into_iter()
        .filter(|binding| {
            keymap::input(map, context, keymap::press(binding.key, binding.modifiers)).is_some()
        })
        .map(|binding| keymap::press(binding.key, binding.modifiers));
    let classic = CLASSIC
        .iter()
        .filter(|(_, meanings)| meanings.iter().any(|(view, _)| *view == context))
        .map(|(letter, _)| keymap::press(Key::Char(*letter), Modifiers::ALT));
    bound.chain(classic).collect()
}

/// The places to press keys at, on `machine` with the key map `map`: in
/// the pools view, each of its rows, which the workspace's tree has more
/// of.
pub(super) fn places(machine: &(Snapshot, Event), map: Map) -> Vec<Place> {
    let mut model = menu(machine, map, Place::Pools(0));
    model.pools.filter.clear();
    let rows = model.pools.matches.len();
    let mut places = vec![
        Place::Startup,
        Place::Main,
        Place::Kernels(0),
        Place::Kernels(1),
        Place::Snapshots,
        Place::Files,
        Place::Packages,
        Place::Log,
    ];
    places.extend((0..rows).map(Place::Pools));
    places.extend((0..7).map(Place::Recovery));
    places
}

/// The kinds reached from every place of the menu on a machine offering
/// `offers`, with each map; it fails when one is not offered.
fn kinds_reached(offers: &[Offer]) -> BTreeSet<Offer> {
    kinds_reached_with(offers, &[Map::Classic, Map::Workspace])
}

/// The kinds reached from every place of the menu on a machine offering
/// `offers`, with each of `maps`; it fails when one is not offered.
fn kinds_reached_with(offers: &[Offer], maps: &[Map]) -> BTreeSet<Offer> {
    let machine = machine(offers);
    let mut kinds = BTreeSet::new();
    for &map in maps {
        for place in places(&machine, map) {
            let context = menu(&machine, map, place).context();
            let filter = [KeyPress::new(Key::Char('/')), KeyPress::new(Key::Enter)];
            let mut sequences: Vec<Vec<KeyPress>> = presses(map, context)
                .into_iter()
                .map(|key| vec![key])
                .collect();
            sequences.push(filter.to_vec());
            for keys in sequences {
                let mut model = menu(&machine, map, place);
                let sent: Vec<Action> = keys
                    .iter()
                    .flat_map(|key| key_press(&mut model, *key))
                    .collect();
                for action in reached(&model, sent) {
                    let at = format!("{offers:?}, {map:?}, {place:?}, {keys:?}: {action:?}");
                    assert!(model.snapshot.is_offered(&action), "{at}");
                    kinds.extend(action.offer());
                }
            }
        }
    }
    kinds
}

#[test]
fn every_key_menu_entry_and_palette_item_leads_only_to_what_is_offered() {
    // Offering everything, the menu reaches every kind.
    let every: BTreeSet<Offer> = Offer::ALL.into_iter().collect();
    assert_eq!(kinds_reached(&Offer::ALL), every);

    assert_eq!(kinds_reached(&[]), BTreeSet::new());
    let app: BTreeSet<Offer> = APP.iter().copied().collect();
    assert_eq!(kinds_reached(APP), app);
    let others: Vec<Offer> = Offer::ALL
        .into_iter()
        .filter(|kind| !APP.contains(kind))
        .collect();
    assert!(
        kinds_reached(&others)
            .iter()
            .all(|kind| others.contains(kind))
    );
}

#[test]
fn each_map_alone_reaches_every_kind_offered() {
    for map in [Map::Classic, Map::Workspace] {
        // The classic map gains no destroy and no command line saved (ADR
        // 0016, point 10).
        let kinds = |offers: &[Offer]| -> BTreeSet<Offer> {
            let is_kept = |kind: &Offer| {
                map == Map::Workspace || !matches!(kind, Offer::Destroy | Offer::CommandLine)
            };
            offers.iter().copied().filter(is_kept).collect()
        };
        let every = kinds(&Offer::ALL);
        assert_eq!(kinds_reached_with(&Offer::ALL, &[map]), every, "{map:?}");
        assert_eq!(kinds_reached_with(APP, &[map]), kinds(APP), "{map:?}");
    }
}

#[test]
fn the_dialogs_of_the_machine_offer_only_what_is_offered() {
    let machine = machine(APP);
    let mut model = menu(&machine, Map::Classic, Place::Main);
    let problem = Event::ImportProblem {
        pool: "backup".parse().unwrap(),
        reason: "foreign".to_owned(),
    };

    events::apply(&mut model, problem);
    let backup = "backup".parse().unwrap();
    let import = |policy| super::dialogs::import(&backup, Access::ReadOnly, policy);
    assert_eq!(
        reached(&model, Vec::new()),
        [
            import(ImportPolicy::Strict),
            import(ImportPolicy::Hostid),
            import(ImportPolicy::Force),
            Action::Shell
        ]
    );
    model.dialog = None;
    events::apply(&mut model, Event::ResealOffered("luks-zroot-a".to_owned()));
    assert_eq!(model.dialog, None);
}

/// Whether `model` was told that the machine does not offer what a key
/// asked for, and opened nothing.
fn is_refused(model: &Model) -> bool {
    model.dialog.is_none()
        && model.palette.is_none()
        && model.notification == Some(Outcome::Warning(text!("not-offered")))
}

/// The demo machine offering everything.
pub(super) fn machine_all() -> (Snapshot, Event) {
    machine(&Offer::ALL)
}

#[test]
fn a_key_of_something_not_offered_opens_nothing_and_says_so() {
    let app = machine(APP);
    let key = |c| KeyPress::new(Key::Char(c));
    // A chroot, unlocking.
    let cases = [
        (Place::Snapshots, key('j')),
        (Place::Main, key('j')),
        (Place::Pools(3), key('u')),
        (Place::Pools(3), KeyPress::new(Key::Enter)),
    ];
    for (place, press) in cases {
        let mut model = menu(&app, Map::Classic, place);
        let screen = model.screen;

        assert_eq!(key_press(&mut model, press), [], "{place:?} {press:?}");
        assert!(is_refused(&model), "{place:?} {press:?}: {model:?}");
        assert_eq!(model.screen, screen);
    }

    // Booting a snapshot, where only an environment's boot is offered.
    let boot_only = machine(&[Offer::Boot]);
    let mut model = menu(&boot_only, Map::Classic, Place::Snapshots);
    assert_eq!(key_press(&mut model, key('b')), []);
    assert!(is_refused(&model), "{model:?}");

    // Offered, the same keys open their dialogs; a volume's when the
    // machine asks for what opens it, and until it withdraws the question.
    let mut model = menu(&machine_all(), Map::Classic, Place::Pools(3));
    let volume = model.selected_volume().cloned().unwrap();
    let sent = key_press(&mut model, key('u'));
    assert!(
        matches!(sent.as_slice(), [Action::Open { device, .. }] if device == volume.device.get()),
        "{sent:?}"
    );
    assert_eq!(model.dialog, None);
    let asked = Event::UnlockRequested {
        volume: volume.name.get().clone(),
        method: crate::menu::world::Method::Passphrase,
        tries_left: Some(3),
        notice: None,
        wait: None,
    };
    events::apply(&mut model, asked);
    assert!(matches!(model.dialog, Some(Dialog::Unlock(_))), "{model:?}");
    events::apply(&mut model, Event::Withdrawn);
    assert_eq!(model.dialog, None);
    let mut model = menu(&app, Map::Classic, Place::Snapshots);
    let _ = key_press(&mut model, key('b'));
    assert!(matches!(model.dialog, Some(Dialog::Name(_))), "{model:?}");
}

#[test]
fn a_volume_does_not_open_again_while_the_machine_works() {
    let mut model = menu(&machine_all(), Map::Classic, Place::Pools(3));
    // As between two questions of the action that opens it.
    model.working = Some(0);
    for press in [KeyPress::new(Key::Char('u')), KeyPress::new(Key::Enter)] {
        assert_eq!(key_press(&mut model, press), [], "{press:?}");
    }
    model.working = None;
    assert!(matches!(
        key_press(&mut model, KeyPress::new(Key::Enter)).as_slice(),
        [Action::Open { .. }]
    ));
}

#[test]
fn the_command_line_does_not_open_a_volume_again_while_the_machine_works() {
    let mut model = menu(&machine_all(), Map::Classic, Place::Pools(3));
    let opens = |model: &Model| {
        let items = palette_items(model);
        let open = |item: &&PaletteItem| matches!(item.then, Then::Act(Action::Open { .. }));
        items.iter().filter(open).count()
    };
    assert!(opens(&model) > 0);
    // As between two questions of the action that opens it.
    model.working = Some(0);
    assert_eq!(opens(&model), 0);
}

#[test]
fn the_toolbar_help_menus_recovery_and_palette_show_only_what_is_offered() {
    let machine = machine(APP);
    let model = menu(&machine, Map::Classic, Place::Snapshots);
    let offered = |intent| keymap::is_offered(Context::Snapshots, intent, &model);
    let help = keymap::help(Map::Classic, Context::Snapshots, &offered, &|intent| {
        keymap::describe(Context::Snapshots, intent)
    });
    let has = |what| help.iter().any(|(_, id)| *id == what);

    assert!(has(crate::text::id!("action-roll-back")));
    assert!(has(crate::text::id!("action-boot")));
    assert!(!has(crate::text::id!("action-chroot")));
    let classic = keymap::classic_bindings(Context::Snapshots, &offered);
    assert!(
        classic
            .iter()
            .all(|(_, id)| *id != crate::text::id!("action-chroot"))
    );

    let mut model = menu(&machine, Map::Classic, Place::Snapshots);
    let _ = key_press(&mut model, KeyPress::new(Key::Enter));
    let Some(Dialog::Choose { choices, .. }) = &model.dialog else {
        panic!("no choice: {:?}", model.dialog);
    };
    let labels: Vec<&str> = choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            text!("snapshot-boot-clone"),
            text!("snapshot-roll-back"),
            text!("snapshot-clone"),
            text!("snapshot-duplicate"),
            text!("snapshot-promote"),
            text!("snapshot-diff"),
        ]
    );

    let model = menu(&machine, Map::Classic, Place::Recovery(0));
    let tools = crate::menu::model::recovery_tools(&model.snapshot);
    assert_eq!(model.recovery.matches.len(), tools.len());
    assert_eq!(tools.len(), 6, "every tool but the chroot");
    let palette = palette_items(&model);
    // Neither a chroot, the tries given back, a volume opened nor one
    // sealed again; the imports' confirmations are offered.
    assert!(palette.iter().all(|item| !matches!(
        item.then,
        Then::Act(Action::Chroot(_) | Action::ResetBootTries(_) | Action::Open { .. })
            | Then::Confirm {
                action: Action::Reseal(_),
                ..
            }
    )));
}

#[test]
fn an_action_not_offered_never_leaves_the_menu() {
    // The countdown boots, though the machine offers no boot.
    let (mut snapshot, _) = machine(&[]);
    snapshot.fallback = None;
    let mut model = Model::new(snapshot, Theme::new(Capabilities::FULL));
    events::start(&mut model);
    assert!(model.countdown.is_some());

    let (model, actions) = super::update(model, super::Message::Tick(60_000));
    assert_eq!(actions, []);
    assert_eq!(
        model.notification,
        Some(Outcome::Warning(text!("not-offered")))
    );
}

#[test]
fn the_power_key_opens_the_firmware_setup_alone_when_only_it_is_offered() {
    let machine = machine(&[Offer::FirmwareSetup]);
    let mut model = menu(&machine, Map::Classic, Place::Main);
    let offered = |intent| keymap::is_offered(Context::Main, intent, &model);
    let help = keymap::help(Map::Classic, Context::Main, &offered, &|intent| {
        keymap::describe(Context::Main, intent)
    });
    assert!(
        help.iter()
            .any(|(_, id)| *id == crate::text::id!("action-power"))
    );

    let _ = key_press(&mut model, KeyPress::new(Key::Char('x')));
    let Some(Dialog::Choose { choices, .. }) = &model.dialog else {
        panic!("no choice: {:?}", model.dialog);
    };
    let labels: Vec<&str> = choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, [text!("power-firmware-setup")]);
}

#[test]
fn enter_on_a_snapshot_is_refused_and_hidden_when_none_of_its_actions_is_offered() {
    let machine = machine(&[Offer::CreateSnapshot, Offer::Power]);
    let mut model = menu(&machine, Map::Classic, Place::Snapshots);
    let offered = |intent| keymap::is_offered(Context::Snapshots, intent, &model);
    let help = keymap::help(Map::Classic, Context::Snapshots, &offered, &|intent| {
        keymap::describe(Context::Snapshots, intent)
    });
    assert!(
        help.iter()
            .all(|(_, id)| *id != crate::text::id!("action-choose"))
    );

    let _ = key_press(&mut model, KeyPress::new(Key::Enter));
    assert!(is_refused(&model), "{model:?}");
}

#[test]
fn files_are_restored_only_from_a_diff_against_the_live_filesystem() {
    let (snapshot, _) = machine(APP);
    let loaded = &snapshot.loaded[0];
    let (old, new) = (
        loaded.snapshots[0].name.clone(),
        loaded.snapshots[1].name.clone(),
    );
    let mut demo = FakeMachine::new(&DemoOptions {
        snapshots: 3,
        ..DemoOptions::default()
    });
    let live = demo.apply(Action::Diff(old.clone(), None)).remove(0);
    let two = demo.apply(Action::Diff(old, Some(new))).remove(0);
    let enter = KeyPress::new(Key::Enter);

    let mut model = menu(&(snapshot.clone(), live), Map::Classic, Place::Files);
    assert!(keymap::is_offered(
        Context::Diff,
        keymap::Intent::Primary,
        &model
    ));
    let _ = key_press(&mut model, enter);
    assert!(
        matches!(
            &model.dialog,
            Some(Dialog::Confirm {
                action: Action::RestoreFiles { .. },
                ..
            })
        ),
        "{:?}",
        model.dialog
    );

    let mut model = menu(&(snapshot, two), Map::Classic, Place::Files);
    assert!(!keymap::is_offered(
        Context::Diff,
        keymap::Intent::Primary,
        &model
    ));
    assert_eq!(key_press(&mut model, enter), []);
    assert!(is_refused(&model), "{model:?}");
}
