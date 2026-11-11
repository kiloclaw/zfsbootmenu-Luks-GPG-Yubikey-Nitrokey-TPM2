use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::*;
use crate::text::id;

/// Every context of the menu. The recovery shell's pager is none: it is
/// no screen of the model and has no help page, and its own tests check
/// its keys (`menu/pager/tests.rs`), and that it has no command line.
pub(super) const CONTEXTS: [Context; 14] = [
    Startup,
    Main,
    Snapshots,
    Kernels,
    Pools,
    Recovery,
    Log,
    Context::Help,
    Diff,
    Packages,
    Context::Choose,
    Context::Text,
    Context::Command,
    Context::Review,
];

/// Every intent offered: the key map's own bindings.
fn every(_: Intent) -> bool {
    true
}

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
    let said = |intent| describe(context, intent);
    footer(Map::Classic, context, width, &every, &said, &|_| false)
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
        footer(
            Map::Classic,
            Context::Choose,
            80,
            &every,
            &|intent| { describe(Context::Choose, intent) },
            &|_| false
        )
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
    let help = help(Map::Classic, Main, &every, &|intent| describe(Main, intent));
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
    let main = classic_bindings(Main, &every);

    assert!(main.contains(&("MOD+K".to_owned(), id!("action-kernels"))));
    assert!(main.contains(&("MOD+R".to_owned(), id!("action-shell"))));
    assert_eq!(classic_bindings(Context::Help, &every), []);
}

#[test]
fn every_action_has_a_title() {
    for context in CONTEXTS {
        let tables = Map::Classic.table().into_iter();
        for binding in tables.chain(Map::Workspace.table()) {
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
        // The classic map's keys the workspace keeps, and its aliases.
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
        // `z` folds a group of the timeline, in the Snapshots tab alone.
        (Snapshots, plain('z'), intent(Intent::Fold)),
        (Main, plain('z'), None),
        (Context::Diff, plain('z'), None),
        // Text fields still type the letters.
        (Context::Text, plain('j'), Some(Input::Type('j'))),
    ];

    for (context, event, expected) in cases {
        assert_eq!(workspace(context, event), expected, "{context:?} {event:?}");
    }
    // The classic map is unchanged, and gains no key.
    assert_eq!(
        input(Map::Classic, Main, plain('k')),
        intent(Intent::Kernels)
    );
    assert_eq!(input(Map::Classic, Snapshots, plain('z')), None);
    assert_eq!(
        input(Map::Classic, Context::Startup, plain('!')),
        intent(Intent::Recovery)
    );
    assert_eq!(input(Map::Classic, Context::Startup, plain('2')), None);
    assert_eq!(
        input(Map::Classic, Main, plain('j')),
        intent(Intent::Chroot)
    );
}

/// Start-up, the Boot tab, binds `Tab` and `Shift-Tab`, the arrows and help,
/// and no character but `?`: a digit or `j` `k` there is the first key of a
/// passphrase, and no key says that what it asks for is not offered. The
/// digits still choose a tab in the other views of start-up.
#[test]
fn start_up_binds_tab_shift_tab_the_arrows_and_help_and_no_character_but_a_question_mark() {
    let workspace = |context, event| input(Map::Workspace, context, event);
    let intent = |intent| Some(Input::Intent(intent));
    let cases = [
        (
            Context::Startup,
            press(Key::Tab, NONE),
            intent(Intent::NextTab),
        ),
        (
            Context::Startup,
            press(Key::BackTab, NONE),
            intent(Intent::PreviousTab),
        ),
        (Context::Startup, press(Key::Up, NONE), intent(Intent::Up)),
        (
            Context::Startup,
            press(Key::Down, NONE),
            intent(Intent::Down),
        ),
        (
            Context::Startup,
            press(Key::Char('n'), CTRL),
            intent(Intent::Down),
        ),
        (Context::Startup, plain('?'), intent(Intent::Help)),
        (
            Context::Startup,
            press(Key::F(1), NONE),
            intent(Intent::Help),
        ),
        (Context::Startup, plain('1'), None),
        (Context::Startup, plain('2'), None),
        (Context::Startup, plain('4'), None),
        (Context::Startup, plain('j'), None),
        (Context::Startup, plain('k'), None),
        (Context::Startup, plain('h'), None),
        (Context::Startup, plain('!'), None),
        (Context::Startup, plain('x'), None),
        (Context::Startup, plain(':'), None),
        (Context::Startup, press(Key::Enter, NONE), None),
        (Context::Startup, press(Key::Escape, NONE), None),
        (
            Context::Startup,
            press(Key::Char('l'), Modifiers::ALT),
            None,
        ),
        // Another tab or help during start-up still takes the digits.
        (Context::Log, plain('1'), intent(Intent::Tab(Tab::Boot))),
        (Pools, plain('4'), intent(Intent::Tab(Tab::Log))),
        (
            Context::Help,
            plain('2'),
            intent(Intent::Tab(Tab::Snapshots)),
        ),
    ];
    for (context, event, expected) in cases {
        assert_eq!(workspace(context, event), expected, "{context:?} {event:?}");
    }
}

/// One meaning per key: in every view with the classic map, but the
/// command line's keys, in each view with the workspace's, whose arrows go
/// into a column in the Boot tab and from segment to segment in the
/// changes. The command line's `Tab` and `Ctrl-R` mean there what they
/// mean nowhere else.
#[test]
fn every_key_has_one_meaning_and_no_letter_depends_on_case() {
    for map in [Map::Classic, Map::Workspace] {
        let table = map.table();
        for (index, binding) in table.iter().enumerate() {
            let is_command_line =
                |binding: &Binding| binding.scope == Scope::Only(&[Context::Command]);
            let twice = table.iter().skip(index + 1).find(|other| {
                let everywhere = map == Map::Classic && !is_command_line(other);
                other.key == binding.key
                    && other.modifiers == binding.modifiers
                    && other.intent != binding.intent
                    && (everywhere || other.scope.has_context_in_common(binding.scope))
            });
            assert!(twice.is_none(), "{map:?}: {binding:?} has two meanings");
            let capital = matches!(binding.key, Key::Char(c) if c.is_uppercase());
            assert!(!capital, "{map:?}: {binding:?} needs Shift");
        }
    }
}

#[test]
fn the_workspace_toolbar_goes_into_the_kernels_with_l() {
    let said = |intent| describe(Main, intent);
    let tools: Vec<(String, String)> = footer(Map::Workspace, Main, 80, &every, &said, &|_| false)
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
    let help = help(Map::Workspace, Main, &every, &|intent| {
        describe(Main, intent)
    });
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
}

#[test]
fn the_workspace_alone_binds_a_key_to_each_tab() {
    let key = |map: Map, tab| map.binding(Intent::Tab(tab)).map(|binding| binding.key);
    assert_eq!(
        Tab::ALL.map(|tab| key(Map::Workspace, tab)),
        ['1', '2', '3', '4'].map(|c| Some(Key::Char(c)))
    );
    assert_eq!(Tab::ALL.map(|tab| key(Map::Classic, tab)), [None; 4]);
}

#[test]
fn the_workspace_s_arrows_go_into_and_out_of_the_kernels_and_still_switch_the_changes() {
    let arrow = |key| press(key, NONE);
    let intent = |intent| Some(Input::Intent(intent));
    let cases = [
        (Main, Key::Right, intent(Intent::Kernels)),
        (Kernels, Key::Left, intent(Intent::Back)),
        (Context::Diff, Key::Right, intent(Intent::Segment)),
        (Context::Diff, Key::Left, intent(Intent::Segment)),
        (Context::Packages, Key::Left, intent(Intent::Segment)),
        (Snapshots, Key::Right, None),
    ];

    for (context, key, expected) in cases {
        let found = input(Map::Workspace, context, arrow(key));
        assert_eq!(found, expected, "{context:?} {key:?}");
    }
    assert_eq!(input(Map::Classic, Main, arrow(Key::Right)), None);
    assert_eq!(
        input(Map::Classic, Context::Diff, arrow(Key::Left)),
        intent(Intent::Segment)
    );
}

#[test]
fn a_review_page_takes_its_name_as_text_and_tab_moves_among_its_buttons() {
    let review = |press| input(Map::Workspace, Context::Review, press);
    let intent = |intent| Some(Input::Intent(intent));
    assert_eq!(review(plain('r')), Some(Input::Type('r')));
    assert_eq!(review(plain('2')), Some(Input::Type('2')));
    assert_eq!(review(plain('?')), Some(Input::Type('?')));
    assert_eq!(review(press(Key::Tab, NONE)), intent(Intent::NextTab));
    assert_eq!(
        review(press(Key::BackTab, NONE)),
        intent(Intent::PreviousTab)
    );
    assert_eq!(review(press(Key::PageDown, NONE)), intent(Intent::PageDown));
    assert_eq!(review(press(Key::Enter, NONE)), intent(Intent::Primary));
    assert_eq!(review(press(Key::Escape, NONE)), intent(Intent::Back));
    // `Home` and `End`, which move in a view, do nothing in its field.
    assert_eq!(review(press(Key::Home, NONE)), None);
    assert_eq!(
        describe(Context::Review, Intent::NextTab),
        id!("action-next-place")
    );
    // The views keep `Tab` for the tabs.
    assert_eq!(
        input(Map::Workspace, Main, press(Key::Tab, NONE)),
        intent(Intent::NextTab)
    );
    assert!(!Scope::Views.contains(Context::Review));
}

#[test]
fn ctrl_r_reveals_in_a_field_and_searches_the_history_in_the_command_line() {
    let ctrl = |c| press(Key::Char(c), CTRL);
    let intent = |intent| Some(Input::Intent(intent));
    for map in [Map::Classic, Map::Workspace] {
        let command = |press| input(map, Context::Command, press);
        assert_eq!(input(map, Context::Text, ctrl('r')), intent(Intent::Reveal));
        assert_eq!(command(ctrl('r')), intent(Intent::History));
        assert_eq!(command(ctrl('w')), intent(Intent::DeleteWord));
        assert_eq!(command(press(Key::Tab, NONE)), intent(Intent::Complete));
        // Letters type, and a field's keys erase.
        assert_eq!(command(plain('k')), Some(Input::Type('k')));
        assert_eq!(command(press(Key::Backspace, NONE)), Some(Input::Erase));
        assert_eq!(command(ctrl('u')), Some(Input::Clear));
        // `Ctrl-K` translates no query, and no `fzf` alias works there.
        assert_eq!(command(ctrl('k')), None);
        assert_eq!(command(press(Key::Char('k'), Modifiers::ALT)), None);
        // The command line's keys mean nothing elsewhere.
        assert_eq!(input(map, Context::Text, ctrl('w')), None);
        assert_eq!(input(map, Context::Text, press(Key::Tab, NONE)), None);
        assert_ne!(input(map, Main, ctrl('r')), intent(Intent::History));
    }
}
