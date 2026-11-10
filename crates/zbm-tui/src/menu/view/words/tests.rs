use pretty_assertions::assert_eq;
use zbm_display::keys::Key;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::theme::Theme;
use crate::menu::world::{Level, Outcome, StageState};

/// The demo in the classic menu (`zbm.menu=classic`), whose keys these
/// tests press, started.
fn started() -> Session {
    let options = DemoOptions {
        classic: true,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session
}

#[test]
fn the_main_view_is_its_title_the_countdown_and_the_environments_in_order() {
    let session = started();
    let model = session.model();

    assert_eq!(title(model), "Boot Environments");
    assert!(countdown(model).is_some_and(|text| text.contains("zroot/ROOT/void")));
    let (rows, numbered) = rows(model);
    assert!(numbered);
    assert!(rows[0].starts_with("zroot/ROOT/void, "), "{rows:?}");
    assert!(rows[0].contains("default"), "{rows:?}");
    assert_eq!(rows.len(), model.environments.matches.len());
    assert!(dialog(model).is_none());
    assert!(
        commands(model)
            .iter()
            .any(|command| command.starts_with("k "))
    );
}

#[test]
fn kernels_snapshots_and_the_log_have_their_rows() {
    let mut session = started();
    session.key(Key::Char('k'));
    let (kernels, numbered) = rows(session.model());
    assert!(numbered);
    assert_eq!(kernels, ["6.12.9_1", "6.12.8_1", "6.8.0_1"]);

    session.key(Key::Escape);
    session.key(Key::Char('s'));
    let (snapshots, numbered) = rows(session.model());
    assert!(numbered && !snapshots.is_empty());
    assert_eq!(snapshots[0].matches(", ").count(), 2, "{snapshots:?}");
}

#[test]
fn a_dialog_is_its_title_lines_and_choices() {
    let mut session = started();
    session.key(Key::Char('s'));
    session.key(Key::Enter);
    let said = dialog(session.model()).expect("the snapshot's actions");
    assert!(!said.title.is_empty());
    assert!(said.choices.len() > 2, "{said:?}");
}

#[test]
fn every_screen_has_its_title_and_rows() {
    let mut session = started();
    // Each view opens by its key, from the main view, and is left with
    // Escape.
    for (key, numbered) in [
        ('p', true),
        ('!', true),
        ('l', false),
        ('?', false),
        ('e', false),
    ] {
        session.key(Key::Char(key));
        let model = session.model();
        assert_ne!(title(model), text!("view-main"), "{key}");
        assert_eq!(rows(model).1, numbered, "{key}");
        session.key(Key::Escape);
    }
    session.key(Key::Char('s'));
    session.key(Key::Char('f'));
    assert_eq!(title(session.model()), text!("view-changes"));
    assert!(rows(session.model()).1);

    let mut model = started().into_model();
    model.screen = Screen::Startup;
    model.snapshot.startup = [
        StageState::Pending,
        StageState::Running,
        StageState::Done,
        StageState::Skipped,
        StageState::Failed("no pool".to_owned()),
    ]
    .into_iter()
    .map(|state| crate::menu::world::Stage {
        name: "Import".to_owned(),
        state,
    })
    .collect();
    assert_eq!(title(&model), text!("view-startup"));
    let (stages, numbered) = rows(&model);
    assert!(!numbered);
    assert_eq!(stages.len(), 5);
    assert!(stages[4].contains("no pool"), "{stages:?}");
    model.screen = Screen::Diff;
    model.diff = None;
    assert_eq!(rows(&model), (Vec::new(), true));
    model.screen = Screen::Kernels;
    model.environments.matches.clear();
    model.snapshot.environments.clear();
    assert_eq!(rows(&model).0, Vec::<String>::new());
    for screen in [Screen::Snapshots, Screen::CommandLine, Screen::Log] {
        model.screen = screen;
        assert!(!title(&model).is_empty());
    }
    model.snapshot.log.entries = [Level::Info, Level::Warning, Level::Error]
        .into_iter()
        .map(|level| crate::menu::world::LogEntry {
            level,
            text: zbm_types::Untrusted::new("said".to_owned()),
        })
        .collect();
    assert_eq!(log(&model).len(), 3);
}

#[test]
fn what_happened_last_is_said_with_its_outcome() {
    let mut model = started().into_model();
    let failed = Outcome::Failed {
        what: "the import".to_owned(),
        why: Vec::new(),
        next: Vec::new(),
    };
    for (outcome, said) in [
        (Outcome::Done("booted".to_owned()), "booted"),
        (Outcome::Warning("odd".to_owned()), "odd"),
        (failed, "the import"),
        (Outcome::Cancelled, ""),
    ] {
        model.notification = Some(outcome);
        let line = notification(&model).unwrap();
        assert!(line.contains(said) && line.contains(": "), "{line}");
    }
    model.working = Some(3000);
    assert_eq!(notification(&model), Some(text!("working", seconds = 3)));
    model.working = None;
    model.notification = None;
    model.progress = Some("Importing".to_owned());
    assert_eq!(notification(&model).as_deref(), Some("Importing"));
}

#[test]
fn confirmations_names_errors_and_the_palette_are_said_too() {
    let mut model = started().into_model();
    let action = crate::menu::world::Action::PowerOff;
    model.dialog = Some(Dialog::Confirm {
        title: "Destroy".to_owned(),
        lines: vec!["It goes.".to_owned()],
        name: "zroot".to_owned(),
        typed: String::new(),
        action,
    });
    let said = dialog(&model).unwrap();
    assert_eq!(said.lines.len(), 2);
    assert!(said.lines[1].contains("zroot"), "{said:?}");
    model.dialog = Some(Dialog::Error {
        what: "It failed".to_owned(),
        why: vec!["no space".to_owned()],
        next: vec!["free some".to_owned()],
    });
    let said = dialog(&model).unwrap();
    assert_eq!(said.title, "It failed");
    assert_eq!(said.lines.len(), 3, "{said:?}");
    model.dialog = Some(Dialog::Error {
        what: "It failed".to_owned(),
        why: Vec::new(),
        next: Vec::new(),
    });
    assert!(dialog(&model).unwrap().lines.is_empty());

    let environment: zbm_types::DatasetName = "zroot/ROOT/void".parse().unwrap();
    let naming = Naming::new(
        crate::menu::model::Make::Snapshot(environment),
        0,
        "Snapshot".to_owned(),
        vec!["Of zroot/ROOT/void".to_owned()],
    );
    model.dialog = Some(Dialog::Name(naming));
    let named = dialog(&model).unwrap();
    assert_eq!(named.lines.len(), 2);
    assert!(
        named.lines[1].starts_with(&text!("name-label")),
        "{named:?}"
    );

    let mut session = started();
    session.key(Key::Char(':'));
    let said = dialog(session.model()).unwrap();
    assert_eq!(said.title, text!("palette-title"));
    assert!(!said.choices.is_empty());
    // The headings of its sections, which are no choices.
    assert_eq!(said.headers.first(), Some(&0));
    assert_eq!(said.choices[0], text!("section-actions"));
    let headings = said.headers.iter().map(|at| said.choices[*at].clone());
    let headings: Vec<String> = headings.collect();
    let expected = [
        text!("section-actions"),
        text!("section-items"),
        text!("section-help"),
    ];
    assert_eq!(headings, expected);
}

#[test]
fn the_command_line_s_help_says_its_command_names_under_their_header() {
    let mut model = started().into_model();
    model.screen = Screen::Help(keymap::Context::Command);
    let (said, _) = rows(&model);
    let header = text!("help-command-names");
    let at = said.iter().position(|row| *row == header);
    let at = at.unwrap_or_else(|| panic!("no header: {said:?}"));
    let boot = text!("command-boot");
    assert!(said[at + 1].starts_with(&format!("{boot}: ")), "{said:?}");
    assert_eq!(said.len() - at - 1, keymap::COMMAND_NAMES.len());
    model.screen = Screen::Help(keymap::Context::Main);
    assert!(!rows(&model).0.contains(&header));
}
