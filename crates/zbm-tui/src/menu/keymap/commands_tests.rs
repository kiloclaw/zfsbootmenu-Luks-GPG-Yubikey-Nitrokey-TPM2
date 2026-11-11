//! Rule 1 of decision 18: what only a character or a chord reaches is a
//! command of the palette, with its keys.

use pretty_assertions::assert_eq;

use super::tests::CONTEXTS;
use super::*;
use Intent::{Diff, Edit, Filter, Fold, Mark, Rewind, Sort};

/// `commands` with keys of their own, to compare.
fn owned(commands: &[(Intent, &str)]) -> Vec<(Intent, String)> {
    commands
        .iter()
        .map(|(intent, keys)| (*intent, (*keys).to_owned()))
        .collect()
}

/// What the palette lists in a view with a map: (intent, keys).
type Listed = (Map, Context, &'static [(Intent, &'static str)]);

/// What the palette lists in the views of each map with more than a
/// filter: what it has no entries of its own for. The classic map's `h`
/// rewinds, and its `Tab` marks.
const EXPECTED: &[Listed] = &[
    (Map::Workspace, Main, &[(Filter, "/")]),
    (
        Map::Workspace,
        Snapshots,
        &[
            (Diff, "f, MOD+D"),
            (Filter, "/"),
            (Sort, "o, MOD+O"),
            (Mark, "Space"),
            (Fold, "z"),
        ],
    ),
    (
        Map::Workspace,
        Kernels,
        &[(Edit, "e, MOD+E"), (Filter, "/")],
    ),
    (Map::Workspace, Pools, &[(Filter, "/"), (Rewind, "MOD+R")]),
    (
        Map::Workspace,
        Context::Diff,
        &[(Filter, "/"), (Mark, "Space")],
    ),
    // The classic map: `h` rewinds, and `Tab` marks.
    (Map::Classic, Main, &[(Filter, "/")]),
    (
        Map::Classic,
        Snapshots,
        &[(Diff, "f, MOD+D"), (Filter, "/"), (Sort, "o, MOD+O")],
    ),
    (Map::Classic, Pools, &[(Rewind, "h, MOD+R"), (Filter, "/")]),
    (Map::Classic, Context::Diff, &[(Filter, "/")]),
];

#[test]
fn the_palette_lists_what_only_a_character_or_a_chord_reaches() {
    for (map, context, list) in EXPECTED {
        assert_eq!(commands(*map, *context), owned(list), "{map:?} {context:?}");
    }
    // The lists of the other views have their filter, and the rest
    // nothing; a text field types, and its chords have no palette there.
    for map in [Map::Classic, Map::Workspace] {
        for context in [Recovery, Packages] {
            assert_eq!(
                commands(map, context),
                owned(&[(Filter, "/")]),
                "{map:?} {context:?}"
            );
        }
        assert_eq!(commands(map, Kernels).len(), 2);
        for context in [Startup, Log, Context::Help, Context::Choose] {
            assert_eq!(commands(map, context), [], "{map:?} {context:?}");
        }
        // A field has chords, as `Ctrl-K`, and lists none of them.
        let layout = input(map, Context::Text, press(Key::Char('k'), CTRL));
        assert_eq!(layout, Some(Input::Intent(Intent::Layout)));
        for context in [Context::Text, Context::Review] {
            assert_eq!(commands(map, context), [], "{map:?} {context:?}");
        }
    }
}

#[test]
fn what_an_arrow_enter_tab_or_delete_reaches_is_not_listed_again() {
    for map in [Map::Classic, Map::Workspace] {
        for context in CONTEXTS {
            for (intent, _) in commands(map, context) {
                let other = map.table().into_iter().find(|binding| {
                    binding.intent == intent
                        && map.is_bound(binding, context)
                        && !matches!(binding.key, Key::Char(_))
                });
                assert_eq!(other.map(|b| b.key), None, "{map:?} {context:?} {intent:?}");
            }
        }
    }
    let has = |map, context, intent| commands(map, context).iter().any(|(i, _)| *i == intent);
    // `Delete` destroys, `Right` opens the kernels, `F1` helps, `Tab`
    // marks in the classic map, and `Enter` opens a volume or a snapshot's
    // actions.
    assert!(!has(Map::Workspace, Snapshots, Intent::Destroy));
    assert!(!has(Map::Workspace, Main, Intent::Kernels));
    assert!(!has(Map::Workspace, Main, Intent::Help));
    assert!(!has(Map::Classic, Snapshots, Intent::Mark));
    assert!(has(Map::Workspace, Snapshots, Intent::Mark));
    for map in [Map::Classic, Map::Workspace] {
        assert!(!has(map, Pools, Intent::Unlock));
        assert!(!has(map, Snapshots, Intent::Rollback));
        assert!(!has(map, Snapshots, Intent::Clone));
        // The palette edits an environment's command line by its name.
        assert!(!has(map, Main, Intent::Edit));
        assert!(has(map, Kernels, Intent::Edit));
    }
}

#[test]
fn digits_only_shorten_the_way_to_a_tab() {
    let mut digits = 0;
    for map in [Map::Classic, Map::Workspace] {
        for context in CONTEXTS {
            for c in '0'..='9' {
                let Some(chosen) = input(map, context, press(Key::Char(c), NONE)) else {
                    continue;
                };
                if is_text(context) {
                    assert_eq!(chosen, Input::Type(c));
                    continue;
                }
                let at = format!("{map:?} {context:?} {c}");
                assert!(matches!(chosen, Input::Intent(Intent::Tab(_))), "{at}");
                // `Tab` and `Shift-Tab` go round the tabs, there too.
                let next = input(map, context, press(Key::Tab, NONE));
                let previous = input(map, context, press(Key::BackTab, NONE));
                assert_eq!(next, Some(Input::Intent(Intent::NextTab)), "{at}");
                assert_eq!(previous, Some(Input::Intent(Intent::PreviousTab)), "{at}");
                digits += 1;
            }
            let tabs = commands(map, context);
            assert!(
                !tabs
                    .iter()
                    .any(|(intent, _)| matches!(intent, Intent::Tab(_)))
            );
        }
    }
    // `1` to `4` in the workspace's views with the tab bar but start-up.
    assert_eq!(digits, 4 * 9);
}

#[test]
fn every_action_bound_to_a_character_has_a_command_name() {
    let mut checked = 0;
    for map in [Map::Classic, Map::Workspace] {
        for context in CONTEXTS.into_iter().filter(|context| !is_text(*context)) {
            let bound = map.table().into_iter();
            let mut intents: Vec<Intent> = bound
                .filter(|binding| map.is_bound(binding, context))
                .filter(|binding| matches!(binding.key, Key::Char(_)) && binding.modifiers == NONE)
                .map(|binding| binding.intent)
                .collect();
            // The classic aliases, chords of a character.
            for (_, meanings) in CLASSIC {
                let meant = meanings.iter().filter(|(view, _)| *view == context);
                intents.extend(meant.map(|(_, intent)| *intent));
            }
            // What moves or chooses, as the workspace's `j` and `k`, is no
            // action; the palette opens the command line; a digit only
            // shortens the way to a tab.
            let commands = intents.into_iter().filter(|intent| !is_move(*intent));
            for intent in commands {
                let at = format!("{map:?} {context:?} {intent:?}");
                let name = command_name(intent).expect(&at);
                assert!(
                    COMMAND_NAMES.iter().any(|(known, _)| *known == name),
                    "{at}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 60, "{checked}");
    // And no two commands share a name.
    let names: Vec<_> = COMMAND_NAMES
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), names.len());
}

/// Whether `intent` moves or chooses, which no command does, or opens the
/// command line, or a tab, which a digit only shortens the way to.
fn is_move(intent: Intent) -> bool {
    use Intent::{
        Back, Down, First, Last, NextTab, PageDown, PageUp, Palette, PreviousTab, Primary, Up,
    };
    matches!(
        intent,
        Up | Down
            | PageUp
            | PageDown
            | First
            | Last
            | Primary
            | Back
            | NextTab
            | PreviousTab
            | Palette
            | Intent::Tab(_)
    )
}

#[test]
fn every_action_a_character_reaches_in_the_workspace_has_its_place_under_rule_1() {
    let map = Map::Workspace;
    let mut checked = 0;
    for context in CONTEXTS.into_iter().filter(|context| !is_text(*context)) {
        let bound = map.table().into_iter();
        let mut intents: Vec<Intent> = bound
            .filter(|binding| map.is_bound(binding, context))
            .filter(|binding| matches!(binding.key, Key::Char(_)))
            .map(|binding| binding.intent)
            .collect();
        for (_, meanings) in CLASSIC {
            let meant = meanings.iter().filter(|(view, _)| *view == context);
            intents.extend(meant.map(|(_, intent)| *intent));
        }
        for intent in intents.into_iter().filter(|intent| !is_move(*intent)) {
            let at = format!("{context:?} {intent:?}");
            let place = reach(context, intent).expect(&at);
            // A snapshot's, a pool's and a volume's are in their menus;
            // `Enter` boots an environment or a kernel.
            let item = matches!(context, Snapshots | Pools)
                || (intent == Intent::Boot && matches!(context, Main | Kernels));
            let is_menu = matches!(place, Reach::Menu | Reach::MenuAndCommand);
            assert!(!is_menu || item, "{at}");
            checked += 1;
        }
    }
    assert!(checked > 60, "{checked}");
    // Where `Enter` boots, the others are the command line's.
    for (context, intent) in [(Main, Intent::NewSnapshot), (Kernels, Intent::Edit)] {
        assert_eq!(reach(context, intent), Some(Reach::CommandLine));
    }
    assert_eq!(reach(Pools, Intent::Rewind), Some(Reach::MenuAndCommand));
    assert_eq!(reach(Snapshots, Intent::Sort), Some(Reach::Command));
}
