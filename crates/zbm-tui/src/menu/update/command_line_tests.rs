//! The command line's keys, history and case (ADR 0007, its keys table):
//! `Tab` completes, `Ctrl-W` deletes a word, `Ctrl-R` shows the entries
//! chosen before, and Caps Lock changes nothing it finds. On the demo
//! machine, under the workspace.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};
use zbm_types::{SnapshotName, Untrusted};

use super::key_press;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::keymap::case_tests::capital;
use crate::menu::keymap::{Context, Modifiers};
use crate::menu::model::{Model, Palette, PaletteItem, Recent, Screen, Then};
use crate::menu::theme::Theme;
use crate::menu::world::Action;
use crate::text::{self, Language, text};

/// The demo under the workspace, started, its countdown stopped.
fn menu() -> Model {
    let mut session = Session::new(&DemoOptions::default(), Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    session.into_model()
}

/// Presses `key` in `model`, and the actions it asks for.
fn press(model: &mut Model, key: KeyPress) -> Vec<Action> {
    key_press(model, key)
}

/// Types `text` into `model`, as typed.
fn type_text(model: &mut Model, text: &str) {
    for c in text.chars() {
        press(model, KeyPress::new(Key::Char(c)));
    }
}

/// `key` with `Ctrl`.
fn ctrl(c: char) -> KeyPress {
    KeyPress::new(Key::Char(c)).with_ctrl()
}

/// The open palette.
fn palette(model: &Model) -> &Palette {
    model.palette.as_ref().expect("the command line is open")
}

/// The labels of what the open palette matches, best first.
fn found(model: &Model) -> Vec<String> {
    let palette = palette(model);
    palette
        .matches
        .iter()
        .map(|ranked| palette.items[ranked.index].label.clone())
        .collect()
}

/// The label of the selected entry of the open palette.
fn selected(model: &Model) -> String {
    found(model)[palette(model).selected].clone()
}

/// Opens the command line, types `query` and chooses the best match.
fn choose(model: &mut Model, query: &str) -> Vec<Action> {
    press(model, KeyPress::new(Key::Char(':')));
    type_text(model, query);
    press(model, KeyPress::new(Key::Enter))
}

#[test]
fn a_query_typed_with_caps_lock_finds_what_it_finds_in_lower_case() {
    for language in [Language::English, Language::Lookalike] {
        text::in_language(language, || {
            let mut lower = menu();
            press(&mut lower, KeyPress::new(Key::Char(':')));
            type_text(&mut lower, "rescan");
            let mut caps = menu();
            press(&mut caps, KeyPress::new(Key::Char(':')));
            for c in "rescan".chars() {
                press(&mut caps, capital(c, Modifiers::NONE));
            }

            assert_eq!(caps.context(), Context::Command);
            // The query shows as typed, and finds the same entries.
            assert_eq!(palette(&caps).query, "RESCAN", "{language:?}");
            assert!(!found(&lower).is_empty(), "{language:?}");
            assert_eq!(found(&caps), found(&lower), "{language:?}");
        });
    }
}

#[test]
fn tab_completes_the_query_to_the_selected_entry() {
    let mut model = menu();
    press(&mut model, KeyPress::new(Key::Char(':')));
    type_text(&mut model, "boot");
    press(&mut model, KeyPress::new(Key::Down));
    let chosen = selected(&model);

    press(&mut model, KeyPress::new(Key::Tab));

    // The command's name and target, which finds the entry again.
    let boot = text!("command-boot");
    let query = palette(&model).query.clone();
    assert!(query.starts_with(&format!("{boot} ")), "{query}");
    assert_eq!(selected(&model), chosen);
    // With nothing found, it leaves the query alone.
    type_text(&mut model, "qqqq");
    press(&mut model, KeyPress::new(Key::Tab));
    assert_eq!(palette(&model).query, format!("{query}qqqq"));
}

#[test]
fn tab_keeps_its_entry_selected_when_another_ranks_as_high() {
    // The snapshots of the selected environment read, on the Boot tab.
    let mut session = Session::new(&DemoOptions::default(), Theme::new(Capabilities::FULL));
    session.start();
    for key in [Key::Escape, Key::Char('2'), Key::Char('1')] {
        session.key(key);
    }
    let mut model = session.into_model();
    press(&mut model, KeyPress::new(Key::Char(':')));
    let boot = |item: &&PaletteItem| matches!(item.then, Then::Act(Action::Boot { .. }));
    let clone = |item: &&PaletteItem| matches!(item.then, Then::BootClone(_));
    let items = &palette(&model).items;
    let environment = items.iter().find(boot).expect("a boot").clone();
    let snapshot = items.iter().find(clone).expect("a snapshot read").clone();
    // Chosen before, the clone's entry comes first under `Ctrl-R`, and the
    // environment's command, which the clone's starts with, finds both in
    // that order.
    for item in [&environment, &snapshot] {
        model.recent.chose(&item.then);
    }
    press(&mut model, ctrl('r'));
    let both = [snapshot.label.clone(), environment.label.clone()];
    assert_eq!(found(&model), both);
    press(&mut model, KeyPress::new(Key::Down));

    press(&mut model, KeyPress::new(Key::Tab));

    assert_eq!(palette(&model).query, environment.also);
    assert_eq!(found(&model), both);
    assert_eq!(selected(&model), environment.label);
}

#[test]
fn ctrl_w_deletes_the_last_word_of_the_query() {
    let mut model = menu();
    press(&mut model, KeyPress::new(Key::Char(':')));
    type_text(&mut model, "boot zroot/ROOT  void  ");
    let query = |model: &Model| palette(model).query.clone();

    press(&mut model, ctrl('w'));
    assert_eq!(query(&model), "boot zroot/ROOT  ");
    press(&mut model, ctrl('w'));
    assert_eq!(query(&model), "boot ");
    press(&mut model, ctrl('w'));
    assert_eq!(query(&model), "");
    press(&mut model, ctrl('w'));
    assert_eq!(query(&model), "");
    // What it finds follows the query, from the top.
    let mut fresh = menu();
    press(&mut fresh, KeyPress::new(Key::Char(':')));
    assert_eq!(found(&model), found(&fresh));
    assert_eq!(palette(&model).selected, 0);
}

#[test]
fn ctrl_r_shows_the_recent_commands_and_the_query_filters_them() {
    let mut model = menu();
    choose(&mut model, "log");
    choose(&mut model, "pools and volumes");
    let log = text!("tool-log");
    let pools = text!("tool-pools");
    press(&mut model, KeyPress::new(Key::Char(':')));

    press(&mut model, ctrl('r'));
    assert!(palette(&model).recent);
    assert_eq!(found(&model), [pools.clone(), log.clone()]);
    let page = crate::menu::lines::page(&model).join("\n");
    assert!(page.contains(&text!("palette-recent")), "{page}");
    type_text(&mut model, "LOG");
    assert_eq!(found(&model), std::slice::from_ref(&log));

    // Again, every entry the query finds.
    press(&mut model, ctrl('r'));
    assert!(!palette(&model).recent);
    assert!(found(&model).contains(&log));
    press(&mut model, ctrl('u'));
    assert_eq!(found(&model).len(), palette(&model).items.len());
}

#[test]
fn a_chosen_command_comes_first_the_next_time_without_duplicates() {
    let mut model = menu();
    choose(&mut model, "log");
    assert_eq!(model.screen, Screen::Log);
    choose(&mut model, "pools and volumes");
    choose(&mut model, "log");
    press(&mut model, KeyPress::new(Key::Char(':')));

    let log = text!("tool-log");
    let pools = text!("tool-pools");
    let after = found(&model);
    assert_eq!(after[..2], [log.clone(), pools.clone()]);
    assert_eq!(model.recent.keys().len(), 2);
    // Each once, and every entry still found.
    let count = |label: &String| after.iter().filter(|l| *l == label).count();
    assert_eq!((count(&log), count(&pools)), (1, 1));
    assert_eq!(after.len(), palette(&model).items.len());
}

#[test]
fn the_history_keeps_twenty_commands_and_forgets_the_oldest() {
    let clone = |index: usize| {
        let name: SnapshotName = format!("zroot/ROOT/void@s{index}").parse().unwrap();
        Then::BootClone(name)
    };
    let mut recent = Recent::default();
    for index in 0..=Recent::MOST {
        recent.chose(&clone(index));
    }

    assert_eq!(recent.keys().len(), Recent::MOST);
    assert_eq!(recent.keys().first(), Some(&clone(Recent::MOST)));
    assert_eq!(recent.keys().last(), Some(&clone(1)));
    assert!(!recent.keys().contains(&clone(0)));
    // Chosen again, one moves to the front, once.
    recent.chose(&clone(5));
    assert_eq!(recent.keys().len(), Recent::MOST);
    assert_eq!(recent.keys().first(), Some(&clone(5)));
}

#[test]
fn the_history_never_holds_what_was_typed() {
    let mut model = menu();
    press(&mut model, KeyPress::new(Key::Char('e')));
    type_text(&mut model, " single");
    press(&mut model, KeyPress::new(Key::Enter));
    let line = Untrusted::new("quiet loglevel=4 single".to_owned());
    let boot = |command_line| {
        Then::Act(Action::Boot {
            environment: "zroot/ROOT/void".parse().unwrap(),
            kernel: None,
            command_line,
            countdown: false,
        })
    };

    let actions = choose(&mut model, "BOOT zr void");
    assert_eq!(
        actions,
        [Action::Boot {
            environment: "zroot/ROOT/void".parse().unwrap(),
            kernel: None,
            command_line: Some(line.clone()),
            countdown: false,
        }]
    );

    // The boot is kept without its line, and the query not at all.
    assert_eq!(model.recent.keys(), [boot(None)]);
    let kept = format!("{:?}", model.recent);
    assert!(
        !kept.contains("single") && !kept.contains("BOOT zr"),
        "{kept}"
    );
    // Found again, alone under `Ctrl-R`, the entry carries the line
    // edited now.
    press(&mut model, KeyPress::new(Key::Char(':')));
    press(&mut model, ctrl('r'));
    let palette = palette(&model);
    let found: Vec<&Then> = palette
        .matches
        .iter()
        .map(|ranked| &palette.items[ranked.index].then)
        .collect();
    assert_eq!(found, [&boot(Some(line))]);
}

#[test]
fn ctrl_k_does_nothing_in_the_command_line() {
    let mut model = menu();
    press(&mut model, KeyPress::new(Key::Char(':')));
    type_text(&mut model, "re");
    let before = palette(&model).clone();
    let layout = model.layout;

    let actions = press(&mut model, ctrl('k'));

    assert_eq!(actions, []);
    assert_eq!(palette(&model), &before);
    assert_eq!(model.layout, layout);
}

#[test]
fn f1_opens_the_help_of_the_command_line_and_esc_comes_back_to_its_query() {
    let mut model = menu();
    press(&mut model, KeyPress::new(Key::Char('2')));
    press(&mut model, KeyPress::new(Key::Char(':')));
    type_text(&mut model, "re");
    press(&mut model, KeyPress::new(Key::Down));
    let before = palette(&model).clone();

    press(&mut model, KeyPress::new(Key::F(1)));
    assert_eq!(model.palette, None);
    assert_eq!(model.screen, Screen::Help(Context::Command));
    assert_eq!(model.context(), Context::Help);
    // The tab stays the one the command line was open on, and the status
    // says where Esc goes back to.
    assert_eq!(model.tab(), crate::menu::keymap::Tab::Snapshots);
    let page = crate::menu::lines::page(&model).join("\n");
    for name in ["Ctrl-W", "Ctrl-R", "Tab", &text!("command-rollback")] {
        assert!(page.contains(name), "{name}: {page}");
    }

    press(&mut model, KeyPress::new(Key::Escape));
    assert_eq!(model.screen, Screen::Snapshots);
    assert_eq!(palette(&model), &before);
    // Its entry opens it too; leaving it by a tab forgets the line.
    press(&mut model, KeyPress::new(Key::Escape));
    let view = text!("view-commands");
    choose(&mut model, &text!("palette-help", view = view.as_str()));
    assert_eq!(model.screen, Screen::Help(Context::Command));
    press(&mut model, KeyPress::new(Key::Char('1')));
    assert_eq!(model.palette_on_help, None);
    press(&mut model, KeyPress::new(Key::Escape));
    assert_eq!(model.palette, None);
}

#[test]
fn the_revert_entry_drops_the_edited_command_line_and_says_so() {
    let mut model = menu();
    let void: zbm_types::DatasetName = "zroot/ROOT/void".parse().unwrap();
    let has_revert = |model: &Model| {
        let items = super::palette_items(model);
        items
            .iter()
            .any(|item| item.then == Then::Revert(void.clone()))
    };
    assert!(!has_revert(&model));
    model
        .edited
        .push((void.clone(), Untrusted::new("quiet".to_owned())));
    assert!(has_revert(&model));

    let name = text!("command-revert");
    assert_eq!(choose(&mut model, &format!("{name} zroot/ROOT/void")), []);
    assert_eq!(model.edited, []);
    let said = text!("command-line-reverted", environment = "zroot/ROOT/void");
    assert_eq!(
        model.notification,
        Some(crate::menu::world::Outcome::Done(said))
    );
    assert_eq!(model.dialog, None);
}

#[test]
fn the_revert_key_says_nothing_when_no_command_line_was_edited() {
    let mut model = menu();
    model.map = crate::menu::keymap::Map::Classic;
    let alt_t = KeyPress::new(Key::Char('t')).with_alt();
    assert_eq!(press(&mut model, alt_t), []);
    assert_eq!(model.notification, None);
    let void: zbm_types::DatasetName = "zroot/ROOT/void".parse().unwrap();
    model
        .edited
        .push((void, Untrusted::new("quiet".to_owned())));
    assert_eq!(press(&mut model, alt_t), []);
    assert_eq!(model.edited, []);
    assert!(model.notification.is_some());
}

#[test]
fn the_log_stays_scrolled_under_the_command_line_s_help() {
    let mut model = menu();
    press(&mut model, KeyPress::new(Key::Char('4')));
    assert_eq!(model.screen, Screen::Log);
    model.scroll = 3;
    press(&mut model, KeyPress::new(Key::Char(':')));
    press(&mut model, KeyPress::new(Key::F(1)));
    assert_eq!(model.scroll, 0);
    press(&mut model, KeyPress::new(Key::Escape));
    assert_eq!(model.screen, Screen::Log);
    assert_eq!(model.scroll, 3);
    assert!(model.palette.is_some());
}

#[test]
fn a_clone_s_boot_is_found_by_its_name_and_tab_names_it() {
    for language in [Language::English, Language::Lookalike] {
        text::in_language(language, || {
            // The snapshots of the selected environment read.
            let options = DemoOptions::default();
            let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
            session.start();
            for key in [Key::Escape, Key::Char('2'), Key::Char('1')] {
                session.key(key);
            }
            let mut model = session.into_model();
            press(&mut model, KeyPress::new(Key::Char(':')));
            let clone = |item: &PaletteItem| match &item.then {
                Then::BootClone(name) => Some((item.clone(), name.clone())),
                _ => None,
            };
            let items = &palette(&model).items;
            let (snapshot, name) = items.iter().find_map(clone).expect("a snapshot read");
            let query = format!("{} {name}", text!("command-boot"));
            // In English the label says the name: no other is built. In the
            // look-alike language it does not, and the name is built.
            let first = snapshot.also.lines().next().unwrap_or_default();
            let built = if language == Language::English {
                ""
            } else {
                &query
            };
            assert_eq!(first, built, "{language:?}");
            type_text(&mut model, &query);
            let palette_now = palette(&model);
            let at = (palette_now.matches.iter())
                .position(|r| palette_now.items[r.index] == snapshot)
                .unwrap_or_else(|| panic!("{language:?}: {query}"));
            model.palette.as_mut().unwrap().selected = at;

            press(&mut model, KeyPress::new(Key::Tab));

            assert_eq!(palette(&model).query, query, "{language:?}");
            assert_eq!(selected(&model), snapshot.label, "{language:?}");
        });
    }
}
