//! The lock's marks (ADR 0010): a locked menu marks with the word of the
//! PIN's cost each tool of the toolbar, next step and command whose key or
//! entry the lock keeps, and the verb of a dry run; never the countdown or
//! the default boot; the shell, which asks for the PIN itself. In both
//! maps, on the demo machine.

use pretty_assertions::assert_eq;
use zbm_display::keys::Key;

use super::is_kept;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::keymap::{self, Intent, Tool};
use crate::menu::model::{Model, Then};
use crate::menu::theme::Theme;
use crate::menu::view::tests::screen;
use crate::menu::view::words;
use crate::menu::world::Action;
use crate::text::{self, text};

/// The demo with the menu `locked`, under the classic map or the
/// workspace, started, its countdown stopped.
fn started(locked: bool, classic: bool) -> Session {
    let options = DemoOptions {
        menu_locked: locked,
        classic,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    session
}

/// The toolbar of `model` at `width` columns.
fn toolbar(model: &Model, width: usize) -> Vec<Tool> {
    let context = model.context();
    let offered = |intent| keymap::is_shown(context, intent, model);
    let said = |intent| keymap::label(context, intent, model);
    let kept = |intent| is_kept(model, context, intent);
    keymap::footer(model.map, context, width, &offered, &said, &kept)
}

/// The tool of `key` in `tools`.
fn tool<'t>(tools: &'t [Tool], key: &str) -> &'t Tool {
    let found = tools.iter().find(|tool| tool.key == key);
    found.unwrap_or_else(|| panic!("no tool {key}: {tools:?}"))
}

/// The command line's entries of `model`, opened, that the lock keeps.
fn kept_entries(session: &mut Session) -> Vec<String> {
    session.key(Key::Char(':'));
    let palette = session
        .model()
        .palette
        .clone()
        .expect("the command line opens");
    session.key(Key::Escape);
    (palette.items.into_iter())
        .filter(|item| item.costs.pin)
        .map(|item| item.label)
        .collect()
}

#[test]
fn a_locked_menu_marks_the_tools_and_commands_the_lock_keeps() {
    let mark = keymap::lock_mark();
    assert_eq!(mark, text!("cost-pin"));
    let mut session = started(true, false);
    // Another environment than the default boots only with the PIN.
    session.key(Key::Down);
    let tools = toolbar(session.model(), 200);
    let enter = text!("key-enter");
    assert_eq!(tool(&tools, &enter).mark.as_deref(), Some(mark.as_str()));
    assert_eq!(tool(&tools, "e").mark.as_deref(), Some(mark.as_str()));
    assert_eq!(tool(&tools, "l").mark, None);
    let shown = screen(session.model(), 120, 40);
    let last = shown.lines().last().unwrap_or_default().to_owned();
    assert!(
        last.contains(&format!(
            "{} {mark}",
            text::title(keymap::describe(keymap::Context::Main, Intent::Edit))
        )),
        "{last}"
    );
    // The inspector's next steps.
    assert!(shown.contains(&format!("edit {mark}")), "{shown}");

    let kept = kept_entries(&mut session);
    assert!(
        kept.contains(&text!("palette-chroot", target = "zroot/ROOT/void")),
        "{kept:?}"
    );
    assert!(!kept.contains(&text!("palette-rescan-pools")), "{kept:?}");
    assert!(!kept.contains(&text!("power-reboot")), "{kept:?}");

    // The dry run's verb.
    session.key(Key::Char(':'));
    session.type_text("export zroot");
    session.key(Key::Enter);
    let shown = screen(session.model(), 120, 40);
    let run = text::title(crate::text::id!("action-run"));
    assert!(shown.contains(&format!("[ {run} {mark} ]")), "{shown}");
}

#[test]
fn a_key_that_opens_a_sheet_is_not_marked_but_its_entries_are() {
    let mark = keymap::lock_mark();
    let mut session = started(true, false);
    session.key(Key::Char('2'));
    let enter = text!("key-enter");
    assert_eq!(tool(&toolbar(session.model(), 200), &enter).mark, None);
    session.key(Key::Enter);
    let said = words::dialog(session.model()).unwrap();
    let roll_back = format!("{} {mark}", text!("snapshot-roll-back"));
    assert!(said.choices.contains(&roll_back), "{said:?}");
    assert!(said.choices.contains(&text!("snapshot-diff")), "{said:?}");
    let shown = screen(session.model(), 120, 40);
    assert!(
        shown.contains(&format!("{}… {mark}", text!("snapshot-roll-back"))),
        "{shown}"
    );
}

#[test]
fn an_open_menu_marks_nothing() {
    let mark = keymap::lock_mark();
    for classic in [false, true] {
        let mut session = started(false, classic);
        session.key(Key::Down);
        let tools = toolbar(session.model(), 200);
        assert!(tools.iter().all(|tool| tool.mark.is_none()), "{tools:?}");
        assert_eq!(kept_entries(&mut session), Vec::<String>::new());
        let shown = screen(session.model(), 120, 40);
        assert!(!shown.contains(&format!(" {mark}")), "{shown}");
        assert!(
            words::commands(session.model())
                .iter()
                .all(|c| !c.ends_with(&mark))
        );
    }
}

#[test]
fn the_default_boot_and_the_countdown_are_never_marked() {
    let mark = keymap::lock_mark();
    // The running system asked to boot `tank/ROOT/arch` once, which is not
    // the default: the countdown boots it without the PIN, though a key
    // that boots it asks for it.
    let options = DemoOptions {
        menu_locked: true,
        next_boot: true,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    let model = session.model();
    assert!(model.countdown.is_some());
    let next = model.selected_environment().unwrap().name.clone();
    assert_ne!(model.snapshot.bootfs.as_ref(), Some(&next));
    let boot = |countdown| Action::Boot {
        environment: next.clone(),
        kernel: None,
        command_line: None,
        countdown,
    };
    assert!(!super::costs_of(model, &Then::Act(boot(true))).pin);
    assert!(super::costs_of(model, &Then::Act(boot(false))).pin);
    let shown = screen(model, 120, 40);
    let counting = (shown.lines())
        .find(|line| line.contains(&next.to_string()) && line.contains(" s"))
        .unwrap_or_else(|| panic!("no countdown: {shown}"));
    assert!(!counting.contains(&mark), "{counting}");

    // The default environment, selected, boots without the PIN.
    let options = DemoOptions {
        menu_locked: true,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    let model = session.model();
    let default = model.selected_environment().unwrap();
    assert_eq!(model.snapshot.bootfs.as_ref(), Some(&default.name));
    let enter = text!("key-enter");
    assert_eq!(tool(&toolbar(model, 200), &enter).mark, None);
    let boot = text!("palette-boot", environment = default.name.to_string());
    assert!(!kept_entries(&mut session).contains(&boot));
}

#[test]
fn a_narrow_command_line_cuts_the_keys_and_costs_before_the_mark() {
    let mark = keymap::lock_mark();
    let mut session = started(true, false);
    session.key(Key::Char('2'));
    session.key(Key::Char(':'));
    session.type_text("roll");
    let roll_back = text!(
        "palette-roll-back",
        snapshot = "zroot/ROOT/void@pre-upgrade"
    );
    let start: String = roll_back.chars().take(9).collect();
    for (width, height) in [(52, 20), (80, 24)] {
        let shown = screen(session.model(), width, height);
        // The row, from its label to the command line's border.
        let row = (shown.lines())
            .find_map(|line| line.split_once(start.as_str()).map(|(_, row)| row))
            .and_then(|row| row.split('│').next())
            .unwrap_or_else(|| panic!("no {start}: {shown}"));
        assert!(
            row.trim_end().ends_with(&format!(" {mark}")),
            "{width}: {row}"
        );
    }
}

#[test]
fn the_shell_is_marked_as_it_asks_for_the_pin() {
    let mut session = started(true, false);
    let model = session.model();
    assert!(is_kept(model, keymap::Context::Main, Intent::Shell));
    assert!(super::costs_of(model, &Then::Act(Action::Shell)).pin);
    assert!(kept_entries(&mut session).contains(&text!("tool-shell.title")));
    // The recovery tools' shell.
    session.key(Key::Char('!'));
    let model = session.model();
    assert_eq!(model.context(), keymap::Context::Recovery);
    assert!(is_kept(model, keymap::Context::Recovery, Intent::Primary));

    let open = started(false, false);
    assert!(!is_kept(open.model(), keymap::Context::Main, Intent::Shell));
}

#[test]
fn the_mark_counts_in_the_toolbar_s_width() {
    let mark = keymap::lock_mark();
    let marked = Tool {
        key: "e".to_owned(),
        label: "Edit".to_owned(),
        is_more: false,
        mark: Some(mark.clone()),
    };
    assert_eq!(marked.width(), "e Edit".len() + 1 + mark.chars().count());

    let locked = started(true, false);
    let open = started(false, false);
    // The width at which the open menu's toolbar just fits: the locked
    // one's, with its marks, does not.
    let tools = toolbar(open.model(), 200);
    let gaps = keymap::TOOLBAR_GAP * tools.len().saturating_sub(1);
    let width = tools.iter().map(Tool::width).sum::<usize>() + gaps;
    assert_eq!(toolbar(open.model(), width).len(), tools.len());
    let fitted = toolbar(locked.model(), width);
    assert!(fitted.len() < tools.len(), "{fitted:?}");
    let used: usize = fitted.iter().map(Tool::width).sum::<usize>()
        + keymap::TOOLBAR_GAP * fitted.len().saturating_sub(1);
    assert!(used <= width, "{used} > {width}");
}

#[test]
fn the_classic_toolbar_and_command_line_are_marked() {
    let mark = keymap::lock_mark();
    let mut session = started(true, true);
    assert_eq!(session.model().map, keymap::Map::Classic);
    let tools = toolbar(session.model(), 200);
    assert_eq!(tool(&tools, "e").mark.as_deref(), Some(mark.as_str()));
    let shown = screen(session.model(), 120, 40);
    assert!(
        shown.lines().last().unwrap_or_default().contains(&mark),
        "{shown}"
    );
    let kept = kept_entries(&mut session);
    assert!(kept.contains(&text!("tool-shell.title")), "{kept:?}");
}

#[test]
fn the_line_view_says_the_keys_costs_and_mark() {
    let mark = keymap::lock_mark();
    let mut session = started(true, false);
    session.key(Key::Down);
    let commands = words::commands(session.model());
    let edit = commands.iter().find(|c| c.starts_with("e "));
    assert!(
        edit.is_some_and(|edit| edit.ends_with(&format!(" {mark}"))),
        "{commands:?}"
    );
    let kernels = commands.iter().find(|c| c.starts_with("l "));
    assert!(
        kernels.is_some_and(|line| !line.contains(&mark)),
        "{commands:?}"
    );

    session.key(Key::Char(':'));
    let said = words::dialog(session.model()).unwrap();
    let dot = session.model().theme.dot();
    let environment = session
        .model()
        .selected_environment()
        .unwrap()
        .name
        .to_string();
    let default = text!("palette-default", environment = environment.as_str());
    let line = said.choices.iter().find(|c| c.starts_with(&default));
    let expected = format!("{default}{dot}d{dot}{} {mark}", text!("cost-writes"));
    assert_eq!(line, Some(&expected), "{said:?}");
}
