//! Rule 2 of decision 18: a capital is its letter, and Caps Lock changes
//! no action, but where letters type.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::tests::CONTEXTS;
use super::*;

/// `c` with Caps Lock on, or with Shift: the character the keyboard sends,
/// with the modifiers held.
fn capital(c: char, modifiers: Modifiers) -> KeyPress {
    let upper = c.to_uppercase().next().unwrap_or(c);
    press(Key::Char(upper), modifiers)
}

#[test]
fn a_capital_does_what_its_letter_does_in_every_view_of_both_maps() {
    let mut checked = 0;
    for map in [Map::Classic, Map::Workspace] {
        for binding in map.table() {
            let Key::Char(c) = binding.key else {
                continue;
            };
            if !c.is_lowercase() {
                continue;
            }
            for context in CONTEXTS.into_iter().filter(|c| map.is_bound(binding, *c)) {
                let letter = input(map, context, press(binding.key, binding.modifiers));
                let shifted = capital(c, binding.modifiers | Modifiers::SHIFT);
                let caps = capital(c, binding.modifiers);
                assert_eq!(
                    input(map, context, shifted),
                    letter,
                    "{map:?} {context:?} {c}"
                );
                assert_eq!(input(map, context, caps), letter, "{map:?} {context:?} {c}");
                assert!(letter.is_some(), "{map:?} {context:?} {binding:?}");
                checked += 1;
            }
        }
    }
    assert!(checked > 50, "{checked} letters checked");
}

#[test]
fn caps_lock_keeps_ctrl_chords_and_their_fzf_aliases_apart() {
    let intent = |intent| Some(Input::Intent(intent));
    let chords = [
        ('c', Intent::Back),
        ('p', Intent::Up),
        ('n', Intent::Down),
        ('l', Intent::Redraw),
    ];
    for map in [Map::Classic, Map::Workspace] {
        for context in [Snapshots, Main] {
            for (letter, meant) in chords {
                for modifiers in [CTRL, CTRL | Modifiers::SHIFT] {
                    let chord = capital(letter, modifiers);
                    assert_eq!(
                        input(map, context, chord),
                        intent(meant),
                        "{map:?} {context:?} {chord:?}"
                    );
                }
            }
        }
        for c in ['c', 'C'] {
            let alias = press(Key::Char(c), Modifiers::ALT);
            assert_eq!(
                input(map, Snapshots, alias),
                intent(Intent::Clone),
                "{map:?} {c}"
            );
        }
    }
}

#[test]
fn a_text_field_types_capitals_and_clears_with_caps_lock() {
    let intent = |intent| Some(Input::Intent(intent));
    for map in [Map::Classic, Map::Workspace] {
        for context in [Context::Text, Context::Review] {
            let typed = |c: char, modifiers| input(map, context, capital(c, modifiers));
            assert_eq!(
                typed('b', NONE),
                Some(Input::Type('B')),
                "{map:?} {context:?}"
            );
            assert_eq!(
                typed('b', Modifiers::SHIFT),
                Some(Input::Type('B')),
                "{map:?} {context:?}"
            );
            assert_eq!(typed('b', CTRL), None, "{map:?} {context:?}");
        }
        let chord = |c: char| input(map, Context::Text, capital(c, CTRL));
        assert_eq!(chord('u'), Some(Input::Clear), "{map:?}");
        assert_eq!(chord('k'), intent(Intent::Layout), "{map:?}");
        assert_eq!(chord('r'), intent(Intent::Reveal), "{map:?}");
    }
}

#[test]
fn a_character_without_one_lower_case_is_looked_up_as_it_is() {
    let typed = |c| press(Key::Char(c), NONE);
    // `İ` is `i` and a combining dot in lower case: two characters.
    for c in ['İ', '?', '7', 'a'] {
        assert_eq!(folded(Main, typed(c)), typed(c), "{c}");
    }
    assert_eq!(folded(Main, typed('A')), typed('a'));
    let shifted = press(Key::Char('?'), Modifiers::SHIFT);
    assert_eq!(folded(Main, shifted), shifted);
}
