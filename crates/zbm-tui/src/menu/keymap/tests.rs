use pretty_assertions::assert_eq;

use super::*;
use crate::text::id;

fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn plain(c: char) -> KeyEvent {
    press(KeyCode::Char(c), NONE)
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
            press(KeyCode::Backspace, NONE),
            Some(Input::Erase),
        ),
        (
            Context::Text,
            press(KeyCode::Char('u'), CTRL),
            Some(Input::Clear),
        ),
    ];

    for (context, event, expected) in cases {
        assert_eq!(input(context, event), expected, "{context:?} {event:?}");
    }
}

#[test]
fn the_keys_of_every_view_work_in_text_fields_too() {
    for context in [Main, Context::Text, Context::Choose] {
        assert_eq!(
            input(context, press(KeyCode::Enter, NONE)),
            Some(Input::Intent(Intent::Primary))
        );
        assert_eq!(
            input(context, press(KeyCode::Esc, NONE)),
            Some(Input::Intent(Intent::Back))
        );
        assert_eq!(
            input(context, press(KeyCode::Char('n'), CTRL)),
            Some(Input::Intent(Intent::Down))
        );
    }
}

#[test]
fn one_key_has_one_meaning_across_views() {
    let mut seen: Vec<(KeyCode, KeyModifiers, Intent)> = Vec::new();

    for binding in KEYMAP {
        let other = seen
            .iter()
            .find(|(code, modifiers, _)| *code == binding.code && *modifiers == binding.modifiers);
        if let Some((_, _, intent)) = other {
            assert_eq!(
                *intent, binding.intent,
                "{:?} has two meanings",
                binding.code
            );
        }
        seen.push((binding.code, binding.modifiers, binding.intent));
    }
}

#[test]
fn no_binding_uses_flow_control_or_keys_that_are_enter() {
    for binding in KEYMAP {
        let forbidden = matches!(binding.code, KeyCode::Char('s' | 'q' | 'j' | 'm' | 'h'))
            && binding.modifiers == CTRL;
        assert!(!forbidden, "{binding:?}");
    }
}

#[test]
fn classic_bindings_keep_their_meaning_in_their_view() {
    let alt = |c| press(KeyCode::Char(c), KeyModifiers::ALT);
    let ctrl = |c| press(KeyCode::Char(c), CTRL);
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
            input(context, event),
            Some(Input::Intent(intent)),
            "{context:?} {event:?}"
        );
    }
    assert_eq!(input(Pools, alt('k')), None);
    assert_eq!(input(Context::Text, alt('k')), None);
}

#[test]
fn ctrl_keys_of_the_new_map_win_over_classic_ones() {
    assert_eq!(
        input(Main, press(KeyCode::Char('p'), CTRL)),
        Some(Input::Intent(Intent::Up))
    );
    assert_eq!(
        input(Snapshots, press(KeyCode::Char('n'), CTRL)),
        Some(Input::Intent(Intent::Down))
    );
    assert_eq!(
        input(Main, press(KeyCode::Char('n'), KeyModifiers::ALT)),
        None
    );
}

#[test]
fn releases_do_nothing() {
    let mut release = press(KeyCode::Enter, NONE);
    release.kind = KeyEventKind::Release;

    assert_eq!(input(Main, release), None);
}

/// The toolbar of `context` at `width` columns, as (key, label).
fn toolbar(context: Context, width: usize) -> Vec<(String, String)> {
    footer(context, width)
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
    assert!(footer(Context::Choose, 80).iter().all(|tool| !tool.is_more));
    assert_eq!(
        toolbar(Context::Diff, 80)[1],
        ("Left/Right".to_owned(), "Files/Packages".to_owned())
    );
}

#[test]
fn help_lists_every_key_of_an_intent_together() {
    let help = help(Main);
    let has = |keys: &str, what| help.contains(&(keys.to_owned(), what));

    assert!(has("Enter", id!("action-boot")));
    assert!(has("b", id!("action-boot")));
    assert!(has("Esc, Ctrl-C", id!("action-back")));
    assert!(has("?, F1", id!("action-help")));
    assert!(has("Up, Ctrl-P", id!("action-select")));
    assert_eq!(key_name(KeyCode::Char(' '), NONE), "Space");
    assert_eq!(key_name(KeyCode::F(1), NONE), "F1");
    assert_eq!(key_name(KeyCode::PageDown, NONE), "PgDn");
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
        for binding in KEYMAP {
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
