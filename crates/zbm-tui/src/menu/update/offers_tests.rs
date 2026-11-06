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
use crate::menu::model::{Dialog, Model, Screen, Then};
use crate::menu::theme::Theme;
use crate::menu::world::{Access, Action, Event, Offer, Outcome, Snapshot, StageState};
use crate::text::text;
use zbm_types::ImportPolicy;

/// What the application offers today.
const APP: &[Offer] = &[
    Offer::Boot,
    Offer::BootSnapshot,
    Offer::SetDefaultEnvironment,
    Offer::DefaultKernel,
    Offer::CreateSnapshot,
    Offer::Rollback,
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
enum Place {
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
fn machine(offers: &[Offer]) -> (Snapshot, Event) {
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
fn menu((snapshot, diff): &(Snapshot, Event), map: Map, place: Place) -> Model {
    let mut model = Model::new(snapshot.clone(), Theme::new(Capabilities::FULL));
    model.map = map;
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
        (Place::Snapshots, Some(name)) => {
            screens::focus(&mut model, name, Screen::Snapshots);
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

/// What `then` asks for, once named or confirmed.
fn then_action(model: &Model, then: &Then) -> Option<Action> {
    match then {
        Then::Act(action) | Then::Confirm { action, .. } => Some(action.clone()),
        Then::Name(naming) => naming.action(),
        Then::Focus(name, Screen::CommandLine) => {
            let environment = model
                .snapshot
                .environments
                .iter()
                .find(|e| &e.name == name)?;
            Some(screens::boot(model, environment, None))
        }
        Then::Open(_) | Then::Focus(..) => None,
    }
}

/// The actions `sent`, and those what is now open can end in: the entries
/// of a menu or of the palette, the action a confirmation or a name
/// dialog asks for, the boot of the command line editor.
fn reached(model: &Model, mut sent: Vec<Action>) -> Vec<Action> {
    match &model.dialog {
        Some(Dialog::Choose { choices, .. }) => {
            sent.extend(choices.iter().filter_map(|c| then_action(model, &c.then)));
        }
        Some(Dialog::Confirm { action, .. }) => sent.push(action.clone()),
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
        let then = Then::Focus(editor.environment.clone(), Screen::CommandLine);
        sent.extend(then_action(model, &then));
    }
    sent
}

/// Every key bound in `context` with `map`: the map's and the classic
/// ones.
fn presses(map: Map, context: Context) -> Vec<KeyPress> {
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

/// The places to press keys at, on the demo machine.
fn places() -> Vec<Place> {
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
    places.extend((0..6).map(Place::Pools));
    places.extend((0..7).map(Place::Recovery));
    places
}

/// The kinds reached from every place of the menu on a machine offering
/// `offers`, with each map; it fails when one is not offered.
fn kinds_reached(offers: &[Offer]) -> BTreeSet<Offer> {
    let machine = machine(offers);
    let mut kinds = BTreeSet::new();
    for map in [Map::Classic, Map::Workspace] {
        for place in places() {
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
fn machine_all() -> (Snapshot, Event) {
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
fn the_toolbar_help_menus_recovery_and_palette_show_only_what_is_offered() {
    let machine = machine(APP);
    let model = menu(&machine, Map::Classic, Place::Snapshots);
    let offered = |intent| keymap::is_offered(Context::Snapshots, intent, &model);
    let help = keymap::help(Map::Classic, Context::Snapshots, &offered);
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
    assert!(palette.iter().all(|item| !matches!(
        item.then,
        Then::Act(Action::Chroot(_) | Action::ResetBootTries(_))
    ) && !matches!(item.then, Then::Confirm { .. })));
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
    let help = keymap::help(Map::Classic, Context::Main, &offered);
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
    let help = keymap::help(Map::Classic, Context::Snapshots, &offered);
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
