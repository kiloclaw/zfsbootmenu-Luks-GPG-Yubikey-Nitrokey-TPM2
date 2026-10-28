//! The characters of a layout by key position, for typing a secret with a
//! layout other than the console's (`docs/dev/menu-design/unlocking.md`,
//! ROADMAP.md UI1): the console types with its layout, and the key a
//! character came from gives the character of the chosen layout.

use kbvm::{Components, Keycode, ModifierMask};

use super::{Layout, accent};

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
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Positions {
    /// By key code: the plain character and the shifted one.
    keys: Vec<(u16, Option<char>, Option<char>)>,
}

impl Positions {
    /// Positions from `(key code, plain, shifted)` triples.
    pub fn from_keys(keys: Vec<(u16, Option<char>, Option<char>)>) -> Positions {
        Positions { keys }
    }

    /// The character the key that gives `c` here gives in `to`, at the
    /// same level; `c` itself when no key here gives it, or that key gives
    /// nothing in `to`.
    pub fn translate(&self, to: &Positions, c: char) -> char {
        let found = self.keys.iter().find_map(|&(code, plain, shifted)| {
            if plain == Some(c) {
                Some((code, false))
            } else if shifted == Some(c) {
                Some((code, true))
            } else {
                None
            }
        });
        let Some((code, shift)) = found else {
            return c;
        };
        to.keys
            .iter()
            .find(|(other, _, _)| *other == code)
            .and_then(|&(_, plain, shifted)| if shift { shifted } else { plain })
            .unwrap_or(c)
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
