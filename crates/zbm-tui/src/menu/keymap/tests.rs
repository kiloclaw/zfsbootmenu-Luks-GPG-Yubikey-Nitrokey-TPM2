use pretty_assertions::assert_eq;

use super::*;

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

#[test]
fn the_toolbar_shows_five_whole_actions_then_more() {
    assert_eq!(
        footer(Main, 80),
        [
            ("Enter", "boot"),
            ("k", "kernels"),
            ("s", "snapshots"),
            ("e", "edit"),
            ("d", "default"),
            ("?", "more"),
        ]
    );
    assert_eq!(
        footer(Main, 40),
        [("Enter", "boot"), ("k", "kernels"), ("?", "more")]
    );
    assert_eq!(footer(Snapshots, 30), [("Enter", "choose"), ("?", "more")]);
    assert_eq!(footer(Main, 5), []);
    assert!(
        footer(Context::Choose, 80)
            .iter()
            .all(|(key, _)| *key != "?")
    );
}

#[test]
fn help_lists_every_key_of_an_intent_together() {
    let help = help(Main);

    assert!(help.contains(&("Enter".to_owned(), "boot")));
    assert!(help.contains(&("b".to_owned(), "boot")));
    assert!(help.contains(&("Esc, Ctrl-C".to_owned(), "back")));
    assert!(help.contains(&("?, F1".to_owned(), "help")));
    assert!(help.contains(&("Up, Ctrl-P".to_owned(), "select")));
    assert_eq!(key_name(KeyCode::Char(' '), NONE), "Space");
    assert_eq!(key_name(KeyCode::F(1), NONE), "F1");
}

#[test]
fn the_classic_bindings_of_a_view_are_listed() {
    let main = classic_bindings(Main);

    assert!(main.contains(&("MOD+K".to_owned(), "kernels")));
    assert!(main.contains(&("MOD+R".to_owned(), "shell")));
    assert_eq!(classic_bindings(Context::Help), []);
}
