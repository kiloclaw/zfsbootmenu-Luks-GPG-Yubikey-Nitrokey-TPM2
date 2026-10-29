use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::*;
use crate::text::id;

fn plain(c: char) -> KeyPress {
    press(Key::Char(c), NONE)
}

#[test]
fn letters_are_actions_in_views_and_typing_in_text_fields() {
    let cases = [
        (Main, plain('k'), Some(Input::Intent(Intent::Kernels))),
        (Main, plain('s'), Some(Input::Intent(Intent::Snapshots))),
        (Snapshots, plain('r'), Some(Input::Intent(Intent::Rollback))),
        (Pools, plain('u'), Some(Input::Intent(Intent::Unlock))),
        (Main, plain('r'), None),
        (Main, plain('z'), None),
        (Context::Text, plain('k'), Some(Input::Type('k'))),
        (Context::Text, plain('?'), Some(Input::Type('?'))),
        (
            Context::Text,
            press(Key::Backspace, NONE),
            Some(Input::Erase),
        ),
        (
            Context::Text,
            press(Key::Char('u'), CTRL),
            Some(Input::Clear),
        ),
    ];

    for (context, event, expected) in cases {
        assert_eq!(
            input(Map::Classic, context, event),
            expected,
            "{context:?} {event:?}"
        );
    }
}

#[test]
fn the_keys_of_every_view_work_in_text_fields_too() {
    for context in [Main, Context::Text, Context::Choose] {
        assert_eq!(
            input(Map::Classic, context, press(Key::Enter, NONE)),
            Some(Input::Intent(Intent::Primary))
        );
        assert_eq!(
            input(Map::Classic, context, press(Key::Escape, NONE)),
            Some(Input::Intent(Intent::Back))
        );
        assert_eq!(
            input(Map::Classic, context, press(Key::Char('n'), CTRL)),
            Some(Input::Intent(Intent::Down))
        );
    }
}

#[test]
fn one_key_has_one_meaning_across_views() {
    let mut seen: Vec<(Key, Modifiers, Intent)> = Vec::new();

    for binding in KEYMAP {
        let other = seen
            .iter()
            .find(|(key, modifiers, _)| *key == binding.key && *modifiers == binding.modifiers);
        if let Some((_, _, intent)) = other {
            assert_eq!(
                *intent, binding.intent,
                "{:?} has two meanings",
                binding.key
            );
        }
        seen.push((binding.key, binding.modifiers, binding.intent));
    }
}

#[test]
fn no_binding_uses_flow_control_or_keys_that_are_enter() {
    for binding in KEYMAP {
        let forbidden = matches!(binding.key, Key::Char('s' | 'q' | 'j' | 'm' | 'h'))
            && binding.modifiers == CTRL;
        assert!(!forbidden, "{binding:?}");
    }
}

#[test]
fn classic_bindings_keep_their_meaning_in_their_view() {
    let alt = |c| press(Key::Char(c), Modifiers::ALT);
    let ctrl = |c| press(Key::Char(c), CTRL);
    let cases = [
        (Main, alt('d'), Intent::SetDefault),
        (Snapshots, alt('d'), Intent::Diff),
        (Kernels, ctrl('d'), Intent::SetDefault),
        (Main, alt('r'), Intent::Shell),
        (Snapshots, ctrl('r'), Intent::Rollback),
        (Snapshots, alt('x'), Intent::CloneAndPromote),
        (Main, alt('k'), Intent::Kernels),
        (Main, alt('K'), Intent::Kernels),
    ];

    for (context, event, intent) in cases {
        assert_eq!(
            input(Map::Classic, context, event),
            Some(Input::Intent(intent)),
            "{context:?} {event:?}"
        );
    }
    assert_eq!(input(Map::Classic, Pools, alt('k')), None);
    assert_eq!(input(Map::Classic, Context::Text, alt('k')), None);
}

#[test]
fn ctrl_keys_of_the_new_map_win_over_classic_ones() {
    assert_eq!(
        input(Map::Classic, Main, press(Key::Char('p'), CTRL)),
        Some(Input::Intent(Intent::Up))
    );
    assert_eq!(
        input(Map::Classic, Snapshots, press(Key::Char('n'), CTRL)),
        Some(Input::Intent(Intent::Down))
    );
    assert_eq!(
        input(Map::Classic, Main, press(Key::Char('n'), Modifiers::ALT)),
        None
    );
}

/// The toolbar of `context` at `width` columns, as (key, label).
fn toolbar(context: Context, width: usize) -> Vec<(String, String)> {
    footer(Map::Classic, context, width)
        .into_iter()
        .map(|tool| (tool.key, tool.label))
        .collect()
}

/// `pairs` as owned strings.
fn owned(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, label)| ((*key).to_owned(), (*label).to_owned()))
        .collect()
}

#[test]
fn the_toolbar_shows_five_whole_actions_then_more() {
    assert_eq!(
        toolbar(Main, 80),
        owned(&[
            ("Enter", "Boot"),
            ("k", "Kernels"),
            ("s", "Snapshots"),
            ("e", "Edit"),
            ("d", "Default"),
            ("?", "More"),
        ])
    );
    assert_eq!(
        toolbar(Main, 40),
        owned(&[("Enter", "Boot"), ("k", "Kernels"), ("?", "More")])
    );
    assert_eq!(
        toolbar(Snapshots, 30),
        owned(&[("Enter", "Choose"), ("?", "More")])
    );
    assert_eq!(toolbar(Main, 5), []);
    assert!(
        footer(Map::Classic, Context::Choose, 80)
            .iter()
            .all(|tool| !tool.is_more)
    );
    assert_eq!(
        toolbar(Context::Diff, 80)[1],
        ("Left/Right".to_owned(), "Files/Packages".to_owned())
    );
}

#[test]
fn help_lists_every_key_of_an_intent_together() {
    let help = help(Map::Classic, Main);
    let has = |keys: &str, what| help.contains(&(keys.to_owned(), what));

    assert!(has("Enter", id!("action-boot")));
    assert!(has("b", id!("action-boot")));
    assert!(has("Esc, Ctrl-C", id!("action-back")));
    assert!(has("?, F1", id!("action-help")));
    assert!(has("Up, Ctrl-P", id!("action-select")));
    assert_eq!(key_name(Key::Char(' '), NONE), "Space");
    assert_eq!(key_name(Key::F(1), NONE), "F1");
    assert_eq!(key_name(Key::PageDown, NONE), "PgDn");
}

#[test]
fn the_classic_bindings_of_a_view_are_listed() {
    let main = classic_bindings(Main);

    assert!(main.contains(&("MOD+K".to_owned(), id!("action-kernels"))));
    assert!(main.contains(&("MOD+R".to_owned(), id!("action-shell"))));
    assert_eq!(classic_bindings(Context::Help), []);
}

#[test]
fn every_action_has_a_title() {
    let contexts = [
        Context::Startup,
        Main,
        Snapshots,
        Kernels,
        Context::Pools,
        Context::Recovery,
        Context::Log,
        Context::Help,
        Context::Diff,
        Context::Packages,
        Context::Choose,
        Context::Text,
    ];
    for context in contexts {
        for binding in KEYMAP.iter().chain(WORKSPACE) {
            let what = describe(context, binding.intent);
            let title = crate::text::title(what);
            assert_ne!(
                title,
                format!("{}.title", what.as_str()),
                "{what:?} has no title"
            );
        }
    }
}

#[test]
fn the_workspace_moves_with_j_k_h_l_and_chooses_tabs_with_digits_and_tab() {
    let workspace = |context, event| input(Map::Workspace, context, event);
    let intent = |intent| Some(Input::Intent(intent));
    let cases = [
        (Main, plain('j'), intent(Intent::Down)),
        (Snapshots, plain('k'), intent(Intent::Up)),
        (Kernels, plain('h'), intent(Intent::Back)),
        (Main, plain('l'), intent(Intent::Kernels)),
        (Main, plain('2'), intent(Intent::Tab(Tab::Snapshots))),
        (Context::Log, plain('1'), intent(Intent::Tab(Tab::Boot))),
        (Pools, press(Key::Tab, NONE), intent(Intent::NextTab)),
        (Main, press(Key::BackTab, NONE), intent(Intent::PreviousTab)),
        // The current map's keys the workspace keeps, and its aliases.
        (Main, plain('s'), intent(Intent::Snapshots)),
        (
            Main,
            press(Key::Char('k'), Modifiers::ALT),
            intent(Intent::Kernels),
        ),
        // What it takes over or frees.
        (Context::Log, plain('l'), None),
        (Main, plain('p'), None),
        (Snapshots, press(Key::Tab, NONE), intent(Intent::NextTab)),
        (Snapshots, plain(' '), intent(Intent::Mark)),
        // Text fields still type the letters, and the start-up view has no
        // tabs.
        (Context::Text, plain('j'), Some(Input::Type('j'))),
        (Context::Startup, plain('2'), None),
    ];

    for (context, event, expected) in cases {
        assert_eq!(workspace(context, event), expected, "{context:?} {event:?}");
    }
    // The current map is unchanged.
    assert_eq!(
        input(Map::Classic, Main, plain('k')),
        intent(Intent::Kernels)
    );
    assert_eq!(
        input(Map::Classic, Main, plain('j')),
        intent(Intent::Chroot)
    );
}

#[test]
fn every_key_has_one_meaning_and_no_letter_depends_on_case() {
    for map in [Map::Classic, Map::Workspace] {
        let table = map.table();
        for (index, binding) in table.iter().enumerate() {
            let twice = table.iter().skip(index + 1).find(|other| {
                other.key == binding.key
                    && other.modifiers == binding.modifiers
                    && other.intent != binding.intent
            });
            assert!(twice.is_none(), "{map:?}: {binding:?} has two meanings");
            let capital = matches!(binding.key, Key::Char(c) if c.is_uppercase());
            assert!(!capital, "{map:?}: {binding:?} needs Shift");
        }
    }
}

#[test]
fn the_workspace_toolbar_goes_into_the_kernels_with_l() {
    let tools: Vec<(String, String)> = footer(Map::Workspace, Main, 80)
        .into_iter()
        .map(|tool| (tool.key, tool.label))
        .collect();
    assert_eq!(
        tools,
        owned(&[
            ("Enter", "Boot"),
            ("l", "Kernels"),
            ("s", "Snapshots"),
            ("e", "Edit"),
            ("d", "Default"),
            ("?", "More"),
        ])
    );
    let help = help(Map::Workspace, Main);
    assert!(help.contains(&("2".to_owned(), id!("action-tab-snapshots"))));
    assert!(help.contains(&("Tab".to_owned(), id!("action-next-tab"))));
    assert!(help.contains(&("Shift-Tab".to_owned(), id!("action-previous-tab"))));
    assert!(help.contains(&("j, Down, Ctrl-N".to_owned(), id!("action-select"))));
}

#[test]
fn the_tabs_go_round() {
    assert_eq!(Tab::Boot.next(), Tab::Snapshots);
    assert_eq!(Tab::Log.next(), Tab::Boot);
    assert_eq!(Tab::Boot.previous(), Tab::Log);
    assert_eq!(Tab::ALL.map(Tab::digit), ['1', '2', '3', '4']);
}
