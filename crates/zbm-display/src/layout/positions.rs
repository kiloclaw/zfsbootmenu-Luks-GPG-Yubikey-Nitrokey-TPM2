//! The characters of a layout by key position, for typing a secret with a
//! layout other than the console's (`docs/dev/menu-design/unlocking.md`,
//! ROADMAP.md UI1): the console types with its layout, and the key a
//! character came from gives the character of the chosen layout. The menu
//! looks a key up by its place on a US keyboard the same way
//! (`docs/dev/menu-design/keymap.md`, rule 3).

use kbvm::{Components, Keycode, ModifierMask};

use super::{Layout, accent};
use crate::keys::{CHARACTERS, LETTERS};

/// The keys of the main block, by Linux key code: the row of digits with
/// the key left of `1`, the rows of `Q`, `A` and `Z`, the key above Enter
/// (43) and the key between the left Shift and `Z` of ISO keyboards (86).
pub const MAIN_BLOCK: &[u16] = &[
    41, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 43,
    30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 86, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53,
];

/// What each key of the main block types, plain and with Shift. A dead key
/// gives the accent it types alone (as followed by the space bar), when
/// the layout has the compose rules of its dead keys.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Positions {
    /// By key code: the plain character and the shifted one.
    keys: Vec<(u16, Option<char>, Option<char>)>,
}

impl Positions {
    /// Positions from `(key code, plain, shifted)` triples.
    pub fn from_keys(keys: Vec<(u16, Option<char>, Option<char>)>) -> Positions {
        Positions { keys }
    }

    /// The positions of the US layout, from the decoder's own tables: there
    /// whether or not the image has the `us` keymap. The key between the
    /// left Shift and `Z` of ISO keyboards (86) types nothing in them.
    pub fn us() -> Positions {
        let key = |code: u16| {
            if let Some(&(_, plain, shifted)) = CHARACTERS.iter().find(|(c, ..)| *c == code) {
                return (code, Some(plain), Some(shifted));
            }
            let letter = LETTERS
                .iter()
                .find_map(|&(first, row)| row.chars().nth(usize::from(code.checked_sub(first)?)));
            (code, letter, letter.map(|c| c.to_ascii_uppercase()))
        };
        Positions {
            keys: MAIN_BLOCK.iter().map(|&code| key(code)).collect(),
        }
    }

    /// The key that gives `c` here, by key code, and whether with Shift.
    /// When two keys give it, the first in the order of [`MAIN_BLOCK`]; a
    /// character of the third level (`AltGr`) or of a key outside the main
    /// block has none.
    pub fn key_of(&self, c: char) -> Option<(u16, bool)> {
        self.keys.iter().find_map(|&(code, plain, shifted)| {
            if plain == Some(c) {
                Some((code, false))
            } else if shifted == Some(c) {
                Some((code, true))
            } else {
                None
            }
        })
    }

    /// The character the key that gives `c` here gives in `to`, at the
    /// same level; `c` itself when no key here gives it, or that key gives
    /// nothing in `to`.
    pub fn translate(&self, to: &Positions, c: char) -> char {
        let Some((code, shift)) = self.key_of(c) else {
            return c;
        };
        to.character_of(code, shift).unwrap_or(c)
    }

    /// The character key `code` gives here, with Shift or without; none for
    /// a key outside the main block or one that types nothing.
    pub fn character_of(&self, code: u16, shift: bool) -> Option<char> {
        self.keys
            .iter()
            .find(|(other, _, _)| *other == code)
            .and_then(|&(_, plain, shifted)| if shift { shifted } else { plain })
    }
}

impl Layout {
    /// What the keys of the main block type in this layout, plain and with
    /// Shift, in its first group and without Caps Lock.
    pub fn positions(&self) -> Positions {
        let group = Components::default().group;
        let level = |code: u16, mods: ModifierMask| {
            let symbol = self
                .lookup
                .lookup(group, mods, Keycode::from_evdev(u32::from(code)))
                .with_ctrl_transform(false)
                .into_iter()
                .next()?;
            symbol.char().or_else(|| {
                let (table, _) = self.dead_keys.as_ref()?;
                accent(table, symbol.keysym())
            })
        };
        Positions {
            keys: MAIN_BLOCK
                .iter()
                .map(|&code| {
                    (
                        code,
                        level(code, ModifierMask::NONE),
                        level(code, ModifierMask::SHIFT),
                    )
                })
                .collect(),
        }
    }
}
