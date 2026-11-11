//! Keys by their place on a US keyboard (rule 3 of
//! `docs/dev/menu-design/keymap.md`, ADR 0007): on the console on the
//! display, a character the key map binds nowhere means what the US
//! character of the same key means, at the same level, so that a Russian
//! keyboard's `л` is `k`. A character the map binds means itself wherever
//! it is typed, and text fields take every character as typed.

use zbm_display::keys::{Key, KeyPress};
use zbm_display::layout::{MAIN_BLOCK, Positions};

use super::{CLASSIC, Context, Input, Map, Modifiers, Scope, folded, input, is_text};

/// The layout a key's character is of and the US layout, by key position.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Typing<'p> {
    /// The layout the character is of.
    pub(crate) layout: &'p Positions,
    /// The US layout.
    pub(crate) us: &'p Positions,
}

/// What `press` means in `context` with the key map `map`, its character
/// of the layout of `typing`, if anything: what [`input`] says, and,
/// outside text fields and start-up, for a character the map binds nowhere
/// with these modifiers, what the US character of its key says, at the
/// level Shift gives. The key is that of its lower case, else its own:
/// Caps Lock on the French digit row types `É`, which no key gives without
/// it; of two keys that give it, the one at that level ([`key_at`]). A
/// character no key of the main block gives (`AltGr`'s, a composed
/// one) and one of the keypad are as typed. Without `typing` (a terminal's
/// characters, the kernel console's, the US layout's), [`input`] alone.
pub(crate) fn input_on(
    map: Map,
    context: Context,
    press: KeyPress,
    typing: Option<Typing<'_>>,
) -> Option<Input> {
    let found = input(map, context, press);
    if found.is_some() || is_text(context) || context == Context::Startup || press.keypad {
        return found;
    }
    let (Some(typing), Key::Char(c), Key::Char(lower)) =
        (typing, press.key, folded(context, press).key)
    else {
        return None;
    };
    if is_bound_anywhere(map, lower, Modifiers::of(&press)) {
        return None;
    }
    let code = [lower, c]
        .into_iter()
        .find_map(|c| key_at(typing.layout, c, press.shift))?;
    let us = typing.us.character_of(code, press.shift)?;
    input(
        map,
        context,
        KeyPress {
            key: Key::Char(us),
            ..press
        },
    )
}

/// The key of `layout` that gives `c`, by key code: the first that gives
/// it at the level `shift` says, else the first that gives it at all. Only
/// the character reaches the menu, not the key: Dvorak's `<` is the ISO
/// key's own and the US `w` key's with Shift, so the ISO key counts as
/// itself and `Shift-w` as `w`; with Shift both give `>`, which counts as
/// the first, the US `e` key.
fn key_at(layout: &Positions, c: char, shift: bool) -> Option<u16> {
    MAIN_BLOCK
        .iter()
        .copied()
        .find(|&code| layout.character_of(code, shift) == Some(c))
        .or_else(|| layout.key_of(c).map(|(code, _)| code))
}

/// Whether `map` binds the character `c` with `modifiers` in some view of
/// the menu: a binding of its table, or, with `Ctrl` or `Alt`, a classic
/// letter. Such a character means itself, wherever it is typed. The
/// recovery shell's pager is no view of the menu: it reads a terminal's
/// characters, which have no place, so its `q` leaves the menu's keys as
/// they were.
fn is_bound_anywhere(map: Map, c: char, modifiers: Modifiers) -> bool {
    let classic = modifiers.has_any(Modifiers::CTRL | Modifiers::ALT)
        && CLASSIC.iter().any(|(letter, _)| *letter == c);
    let pager = Scope::Only(&[Context::Pager]);
    classic
        || map.table().into_iter().any(|binding| {
            binding.key == Key::Char(c) && binding.modifiers == modifiers && binding.scope != pager
        })
}
