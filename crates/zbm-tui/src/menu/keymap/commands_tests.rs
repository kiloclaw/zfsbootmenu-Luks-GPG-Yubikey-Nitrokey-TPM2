//! Rule 1 of decision 18: what only a character or a chord reaches is a
//! command of the palette, with its keys.

use pretty_assertions::assert_eq;

use super::tests::CONTEXTS;
use super::*;
use Intent::{
    Chroot, Diff, Edit, Filter, Fold, Import, InheritDefault, Mark, NewSnapshot, ReadWrite,
    RevertCommandLine, Rewind, SetDefault, Sort,
};

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
/// filter. The classic map's `j` chroots, its `h` rewinds, and its `Tab`
/// marks.
const EXPECTED: &[Listed] = &[
    (
        Map::Workspace,
        Main,
        &[
            (SetDefault, "d, MOD+D"),
            (NewSnapshot, "n"),
            (ReadWrite, "w, MOD+W"),
            (Filter, "/"),
            (RevertCommandLine, "MOD+T"),
            (Chroot, "MOD+J"),
        ],
    ),
    (
        Map::Workspace,
        Snapshots,
        &[
            (Diff, "f, MOD+D"),
            (NewSnapshot, "n, Alt+N"),
            (Filter, "/"),
            (Sort, "o, MOD+O"),
            (Mark, "Space"),
            (Fold, "z"),
        ],
    ),
    (
        Map::Workspace,
        Kernels,
        &[
            (Edit, "e, MOD+E"),
            (SetDefault, "d, MOD+D"),
            (Filter, "/"),
            (InheritDefault, "MOD+U"),
        ],
    ),
    (
        Map::Workspace,
        Pools,
        &[
            (Import, "i"),
            (ReadWrite, "w"),
            (Filter, "/"),
            (Rewind, "MOD+R"),
        ],
    ),
    (
        Map::Workspace,
        Context::Diff,
        &[(Filter, "/"), (Mark, "Space")],
    ),
    // The classic map: `j` chroots, `h` rewinds, and `Tab` marks.
    (
        Map::Classic,
        Main,
        &[
            (SetDefault, "d, MOD+D"),
            (NewSnapshot, "n"),
            (ReadWrite, "w, MOD+W"),
            (Chroot, "j, MOD+J"),
            (Filter, "/"),
            (RevertCommandLine, "MOD+T"),
        ],
    ),
    (
        Map::Classic,
        Snapshots,
        &[
            (Diff, "f, MOD+D"),
            (NewSnapshot, "n, Alt+N"),
            (Filter, "/"),
            (Sort, "o, MOD+O"),
        ],
    ),
    (
        Map::Classic,
        Pools,
        &[
            (Import, "i"),
            (ReadWrite, "w"),
            (Rewind, "h, MOD+R"),
            (Filter, "/"),
        ],
    ),
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
        assert_eq!(commands(map, Kernels).len(), 4);
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
