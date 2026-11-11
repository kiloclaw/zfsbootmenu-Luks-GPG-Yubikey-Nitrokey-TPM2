//! Tests of the command line saved for every boot (ADR 0016, point 18),
//! the workspace's: its two commands for each environment, the save
//! editor and its dry run, which sends nothing before its `Enter`; the
//! lines it refuses; the edit for this boot it replaces; `e`, which still
//! edits for one boot; and the classic map, which has neither command.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};
use zbm_types::{DatasetName, Untrusted};

use super::{Message, update};
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine, Session};
use crate::menu::model::{Dialog, Editor, Model, PaletteItem, Reviewing, Screen, Then};
use crate::menu::theme::Theme;
use crate::menu::world::{Action, CommandLine, Event, Origin, Outcome};
use crate::text::text;

fn theme() -> Theme {
    Theme::new(Capabilities::FULL)
}

fn dataset(name: &str) -> DatasetName {
    name.parse().unwrap()
}

/// The demo under `options`, started, its countdown stopped.
fn started(options: &DemoOptions) -> Session {
    let mut session = Session::new(options, theme());
    session.start();
    session.key(Key::Escape);
    session
}

/// `session` after `:`, `label` typed and `Enter`: the entry the label
/// names ranks first.
fn chosen(session: &mut Session, label: &str) {
    session.key(Key::Char(':'));
    session.type_text(label);
    session.key(Key::Enter);
}

/// The save editor of `environment` open in `session`.
fn save_editor(session: &mut Session, environment: &str) {
    chosen(session, &format!("Set the Command Line of {environment}"));
}

/// The page of the dialog of `model`.
fn page(model: &Model) -> &Reviewing {
    match &model.dialog {
        Some(Dialog::Review(reviewing)) => reviewing,
        other => panic!("no review page: {other:?}"),
    }
}

/// The command line's entries of `model`, as `:` lists them.
fn entries(model: Model) -> Vec<PaletteItem> {
    let (model, _) = update(model, Message::Key(KeyPress::new(Key::Char(':'))));
    model.palette.expect("the command line opens").items
}

/// The line of `environment` on the machine of `model`, and its origin.
fn own_line(model: &Model, environment: &str) -> (String, Origin) {
    let found = (model.snapshot.environments.iter())
        .find(|found| found.name.as_str() == environment)
        .expect("the environment is the demo's");
    let line = &found.command_line;
    (line.value.get().clone(), line.origin.clone())
}

#[test]
fn the_save_editor_shows_its_dry_run_and_sends_nothing_before_enter() {
    let mut session = started(&DemoOptions::default());
    save_editor(&mut session, "tank/ROOT/arch");

    let model = session.model();
    assert_eq!(model.screen, Screen::CommandLine);
    let editor = model.editor.as_ref().expect("the editor opens");
    assert!(editor.save);
    assert_eq!(editor.text, "rw quiet splash");

    session.type_text(" single");
    session.key(Key::Enter);
    let line = Untrusted::new("rw quiet splash single".to_owned());
    let saved = Action::SetCommandLine(dataset("tank/ROOT/arch"), line);
    let reviewing = page(session.model());
    assert_eq!(reviewing.action, saved);
    assert!(reviewing.typed.is_none() && reviewing.is_ready());
    // The plan only reads: nothing is recorded as sent.
    assert_eq!(session.actions, []);

    // `Esc` goes back to the line, as typed.
    session.key(Key::Escape);
    let model = session.model();
    assert!(model.dialog.is_none());
    assert_eq!(model.screen, Screen::CommandLine);
    assert_eq!(
        model.editor.as_ref().map(|e| e.text.as_str()),
        Some("rw quiet splash single")
    );
    assert_eq!(session.actions, []);

    session.key(Key::Enter);
    session.key(Key::Enter);
    assert_eq!(session.actions, [saved]);
    let model = session.model();
    assert!(model.editor.is_none() && model.dialog.is_none());
    assert_eq!(model.screen, Screen::Main);
    let done = "the command line of tank/ROOT/arch is saved (tank imported read/write)";
    assert_eq!(model.notification, Some(Outcome::Done(done.to_owned())));
    assert_eq!(
        own_line(model, "tank/ROOT/arch"),
        ("rw quiet splash single".to_owned(), Origin::Local)
    );
}

#[test]
fn an_empty_line_saved_lets_the_environment_inherit_it() {
    let mut session = started(&DemoOptions::default());
    save_editor(&mut session, "tank/ROOT/arch");

    session.press(KeyPress::new(Key::Char('u')).with_ctrl());
    session.key(Key::Enter);

    let inherit = Action::InheritCommandLine(dataset("tank/ROOT/arch"));
    assert_eq!(page(session.model()).action, inherit);
}

#[test]
fn the_save_editor_refuses_a_line_the_machine_would_not_save() {
    let mut model = started(&DemoOptions::default()).into_model();
    let editor = |text: &str| Editor {
        environment: dataset("tank/ROOT/arch"),
        kernel: None,
        text: text.to_owned(),
        save: true,
    };
    let control = text!("command-line-control");
    let long = text!("command-line-too-long", longest = 8191);
    for (text, refused) in [
        ("quiet\tsplash", Some(control.clone())),
        ("quiet\u{1b}[2J", Some(control)),
        (&"x".repeat(8192), Some(long)),
        (&"x".repeat(8191), None),
    ] {
        model.dialog = None;
        model.notification = None;
        model.screen = Screen::CommandLine;
        model.editor = Some(editor(text));

        let (after, actions) = update(model, Message::Key(KeyPress::new(Key::Enter)));
        model = after;

        match refused {
            Some(why) => {
                assert_eq!(actions, [], "{text:?}");
                assert!(model.dialog.is_none(), "{text:?}");
                assert_eq!(model.notification, Some(Outcome::Warning(why)));
                assert_eq!(model.editor.as_ref().map(|e| e.text.as_str()), Some(text));
            }
            None => assert!(
                matches!(actions.as_slice(), [Action::Plan(_)]),
                "{actions:?}"
            ),
        }
    }
}

#[test]
fn a_saved_line_drops_the_edit_for_this_boot() {
    let mut model = started(&DemoOptions::default()).into_model();
    let edited = Untrusted::new("quiet loglevel=4 single".to_owned());
    model.edited = vec![
        (dataset("zroot/ROOT/void"), edited.clone()),
        (dataset("tank/ROOT/arch"), Untrusted::new("rw".to_owned())),
    ];
    let mut session = Session::with_model(FakeMachine::new(&DemoOptions::default()), model);

    // The save starts from the line edited for this boot.
    save_editor(&mut session, "zroot/ROOT/void");
    let editor = session.model().editor.as_ref().expect("the editor opens");
    assert_eq!(editor.text, edited.get().as_str());
    session.key(Key::Enter);
    session.key(Key::Enter);

    let model = session.model();
    let left: Vec<&str> = model.edited.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(left, ["tank/ROOT/arch"]);
    assert_eq!(
        own_line(model, "zroot/ROOT/void"),
        (edited.get().clone(), Origin::Local)
    );

    // Inheriting it drops it too.
    chosen(&mut session, "Let tank/ROOT/arch Inherit Its Command Line");
    session.key(Key::Enter);
    let inherit = Action::InheritCommandLine(dataset("tank/ROOT/arch"));
    assert_eq!(session.actions.last(), Some(&inherit));
    let model = session.model();
    assert!(model.edited.is_empty(), "{:?}", model.edited);
    assert_eq!(
        own_line(model, "tank/ROOT/arch"),
        (String::new(), Origin::Default)
    );
}

fn finished(outcome: Outcome) -> Message {
    Message::Event(Box::new(Event::Finished(outcome)))
}

fn enter() -> Message {
    Message::Key(KeyPress::new(Key::Enter))
}

/// The machine of `model` once it saved `line` on `environment`.
fn changed(model: &Model, environment: &DatasetName, line: &Untrusted<String>) -> Message {
    let mut snapshot = model.snapshot.clone();
    for found in &mut snapshot.environments {
        if found.name == *environment {
            found.command_line = CommandLine {
                value: line.clone(),
                origin: Origin::Local,
                expanded: line.clone(),
            };
        }
    }
    Message::Event(Box::new(Event::Changed(Box::new(snapshot))))
}

/// The menu with `zroot/ROOT/void` edited for this boot as `edited`, its
/// save editor open from that line, and its dry run planned.
fn save_planned(edited: &Untrusted<String>) -> Model {
    let mut model = started(&DemoOptions::default()).into_model();
    model.edited = vec![(dataset("zroot/ROOT/void"), edited.clone())];
    let mut session = Session::with_model(FakeMachine::new(&DemoOptions::default()), model);
    save_editor(&mut session, "zroot/ROOT/void");
    session.key(Key::Enter);
    assert!(page(session.model()).is_ready());
    session.into_model()
}

#[test]
fn only_the_save_done_closes_its_editor_and_drops_the_edit_for_this_boot() {
    let void = dataset("zroot/ROOT/void");
    let edited = Untrusted::new("quiet loglevel=4 single".to_owned());
    let snapshot_taken = || finished(Outcome::Done("snapshot taken".to_owned()));
    let is_open = |model: &Model| {
        model.screen == Screen::CommandLine && model.editor.as_ref().is_some_and(|e| e.save)
    };
    // Another change done while the line is typed leaves its editor.
    let mut session = started(&DemoOptions::default());
    save_editor(&mut session, "zroot/ROOT/void");
    session.type_text(" single");
    let (typing, _) = update(session.into_model(), snapshot_taken());
    assert!(is_open(&typing));
    assert_eq!(
        typing.editor.map(|editor| editor.text),
        Some("quiet loglevel=4 single".to_owned())
    );

    let mut model = save_planned(&edited);
    let actions;
    (model, actions) = update(model, enter());
    let saved = Action::SetCommandLine(void.clone(), edited.clone());
    assert_eq!(actions, std::slice::from_ref(&saved));
    // Asked for, not done: the edit for this boot stays.
    assert_eq!(model.edited, [(void.clone(), edited.clone())]);
    assert_eq!(model.saving, Some(saved));

    // Another change done meanwhile: its machine shows the old line.
    (model, _) = update(model, snapshot_taken());
    assert!(is_open(&model));
    assert_eq!(model.edited.len(), 1);

    let saved = changed(&model, &void, &edited);
    (model, _) = update(model, saved);
    (model, _) = update(model, finished(Outcome::Done("saved".to_owned())));
    assert!(model.edited.is_empty(), "{:?}", model.edited);
    assert_eq!(model.saving, None);
    assert!(model.editor.is_none());
    assert_eq!(model.screen, Screen::Main);
}

#[test]
fn a_save_that_fails_keeps_its_editor_and_the_edit_for_this_boot() {
    let void = dataset("zroot/ROOT/void");
    let edited = Untrusted::new("quiet loglevel=4 single".to_owned());
    let mut model = save_planned(&edited);
    let failed = Outcome::Failed {
        what: "unable to save the command line of zroot/ROOT/void".to_owned(),
        why: vec!["zroot may not be imported read/write".to_owned()],
        next: Vec::new(),
    };

    (model, _) = update(model, enter());
    (model, _) = update(model, finished(failed));
    model.dialog = None;

    assert_eq!(model.screen, Screen::CommandLine);
    let editor = model.editor.as_ref().expect("the editor stays");
    assert!(editor.save);
    assert_eq!(editor.text, edited.get().as_str());
    assert_eq!(model.edited, [(void, edited)]);
    // `Esc` leaves it, and the edit for this boot boots.
    (model, _) = update(model, Message::Key(KeyPress::new(Key::Escape)));
    assert_eq!(model.screen, Screen::Main);
    assert_eq!(model.edited.len(), 1);
}

#[test]
fn an_edit_for_this_boot_made_after_a_save_was_asked_for_stays() {
    let void = dataset("zroot/ROOT/void");
    let mut model = started(&DemoOptions::default()).into_model();
    let line = Untrusted::new("quiet".to_owned());
    model.saving = Some(Action::SetCommandLine(void.clone(), line.clone()));
    model.screen = Screen::CommandLine;
    model.editor = Some(Editor {
        environment: void.clone(),
        kernel: None,
        text: "quiet single".to_owned(),
        save: false,
    });

    (model, _) = update(model, enter());
    assert_eq!(model.saving, None);
    let saved = changed(&model, &void, &line);
    (model, _) = update(model, saved);
    (model, _) = update(model, finished(Outcome::Done(String::new())));

    let kept = Untrusted::new("quiet single".to_owned());
    assert_eq!(model.edited, [(void, kept)]);
}

#[test]
fn e_still_edits_for_one_boot() {
    let mut session = started(&DemoOptions::default());
    session.key(Key::Char('e'));

    let editor = session.model().editor.as_ref().expect("the editor opens");
    assert!(!editor.save);
    session.type_text(" single");
    session.key(Key::Enter);

    assert!(session.model().dialog.is_none());
    let [Action::Boot { command_line, .. }] = session.actions.as_slice() else {
        panic!("not a boot: {:?}", session.actions);
    };
    let line = command_line.as_ref().map(|line| line.get().as_str());
    assert_eq!(line, Some("quiet loglevel=4 single"));
}

#[test]
fn inherit_is_offered_only_for_a_line_set_here() {
    let model = started(&DemoOptions::default()).into_model();

    let items = entries(model);

    let named = |is_it: fn(&Then) -> Option<&DatasetName>| -> Vec<String> {
        (items.iter())
            .filter_map(|item| is_it(&item.then).map(ToString::to_string))
            .collect()
    };
    let saves = named(|then| match then {
        Then::SaveCommandLine(name) => Some(name),
        _ => None,
    });
    assert_eq!(
        saves,
        [
            "zroot/ROOT/void",
            "zroot/ROOT/ubuntu",
            "tank/ROOT/arch",
            "tank/ROOT/debian"
        ]
    );
    // Only `tank/ROOT/arch` sets its own line.
    let inherits = named(|then| match then {
        Then::Act(Action::InheritCommandLine(name)) => Some(name),
        _ => None,
    });
    assert_eq!(inherits, ["tank/ROOT/arch"]);
}

#[test]
fn the_cost_column_says_the_pin_and_the_read_write_import_of_both() {
    for menu_locked in [false, true] {
        let options = DemoOptions {
            menu_locked,
            ..DemoOptions::default()
        };
        let items = entries(started(&options).into_model());
        let costs = |is_it: &dyn Fn(&Then) -> bool| {
            let item = items.iter().find(|item| is_it(&item.then)).unwrap();
            (item.costs.pin, item.costs.writes, item.costs.key)
        };
        let void = dataset("zroot/ROOT/void");
        let arch = dataset("tank/ROOT/arch");
        // The demo's pools are imported read-only; no key is loaded for a
        // property.
        let save = costs(&|then| *then == Then::SaveCommandLine(void.clone()));
        assert_eq!(save, (menu_locked, true, false));
        let save = costs(&|then| *then == Then::SaveCommandLine(arch.clone()));
        let inherit = Then::Act(Action::InheritCommandLine(arch.clone()));
        assert_eq!(save, (menu_locked, true, false));
        assert_eq!(costs(&|then| *then == inherit), save);
    }
}

#[test]
fn the_classic_map_has_no_persistent_edit() {
    let options = DemoOptions {
        classic: true,
        ..DemoOptions::default()
    };
    let mut session = started(&options);

    let saves = entries(started(&options).into_model())
        .into_iter()
        .any(|item| {
            matches!(
                item.then,
                Then::SaveCommandLine(_) | Then::Act(Action::InheritCommandLine(_))
            )
        });
    assert!(!saves);
    session.key(Key::Char('e'));
    let editor = session.model().editor.as_ref().expect("the editor opens");
    assert!(!editor.save);
}

#[test]
fn a_save_the_machine_does_not_offer_opens_no_dry_run() {
    let mut model = started(&DemoOptions::default()).into_model();
    model
        .snapshot
        .offers
        .remove(&crate::menu::world::Offer::CommandLine);
    model.screen = Screen::CommandLine;
    model.editor = Some(Editor {
        environment: dataset("tank/ROOT/arch"),
        kernel: None,
        text: "rw".to_owned(),
        save: true,
    });

    let (model, actions) = update(model, Message::Key(KeyPress::new(Key::Enter)));

    assert_eq!(actions, []);
    assert!(model.dialog.is_none());
    let refused = Outcome::Warning(text!("not-offered"));
    assert_eq!(model.notification, Some(refused));
}
