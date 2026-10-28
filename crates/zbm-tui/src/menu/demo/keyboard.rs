//! The keyboard layouts of the demo machine, which has no keymaps to load:
//! their characters by key position, as the image's XKB keymaps give them
//! to a real menu (`zbm_app::read::keyboard`). Only the keys of the main
//! block are listed; translation leaves the others unchanged.

use std::collections::BTreeMap;

use zbm_display::layout::{MAIN_BLOCK, Positions};

/// The rows of each layout, unshifted and shifted, in the order of
/// [`MAIN_BLOCK`] without the key between Shift and `Z` of ISO keyboards
/// (86): the same index is the same key in every layout.
const LAYOUTS: &[(&str, [&str; 8])] = &[
    (
        "us",
        [
            "`1234567890-=",
            "~!@#$%^&*()_+",
            "qwertyuiop[]\\",
            "QWERTYUIOP{}|",
            "asdfghjkl;'",
            "ASDFGHJKL:\"",
            "zxcvbnm,./",
            "ZXCVBNM<>?",
        ],
    ),
    (
        "de",
        [
            "^1234567890ß´",
            "°!\"§$%&/()=?`",
            "qwertzuiopü+#",
            "QWERTZUIOPÜ*'",
            "asdfghjklöä",
            "ASDFGHJKLÖÄ",
            "yxcvbnm,.-",
            "YXCVBNM;:_",
        ],
    ),
    (
        "fr",
        [
            "²&é\"'(-è_çà)=",
            "²1234567890°+",
            "azertyuiop^$*",
            "AZERTYUIOP¨£µ",
            "qsdfghjklmù",
            "QSDFGHJKLM%",
            "wxcvbn,;:!",
            "WXCVBN?./§",
        ],
    ),
    (
        "gb",
        [
            "`1234567890-=",
            "¬!\"£$%^&*()_+",
            "qwertyuiop[]#",
            "QWERTYUIOP{}~",
            "asdfghjkl;'",
            "ASDFGHJKL:@",
            "zxcvbnm,./",
            "ZXCVBNM<>?",
        ],
    ),
];

/// The layouts of the demo machine, the console's own first.
pub fn names() -> Vec<String> {
    LAYOUTS.iter().map(|(name, _)| (*name).to_owned()).collect()
}

/// The characters by key position of every layout of the demo machine.
pub fn positions() -> BTreeMap<String, Positions> {
    LAYOUTS
        .iter()
        .map(|(name, rows)| ((*name).to_owned(), from_rows(rows)))
        .collect()
}

/// The positions of the layout whose rows are `rows`.
fn from_rows(rows: &[&str; 8]) -> Positions {
    let codes = MAIN_BLOCK.iter().copied().filter(|&code| code != 86);
    let plain = rows.iter().step_by(2).flat_map(|row| row.chars());
    let shifted = rows.iter().skip(1).step_by(2).flat_map(|row| row.chars());
    Positions::from_keys(
        codes
            .zip(plain.zip(shifted))
            .map(|(code, (plain, shifted))| (code, Some(plain), Some(shifted)))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layout_has_rows_of_the_same_lengths() {
        let lengths = |rows: &[&str; 8]| rows.map(|row| row.chars().count());
        let us = lengths(&LAYOUTS[0].1);
        for (name, rows) in LAYOUTS {
            assert_eq!(lengths(rows), us, "{name}");
        }
        assert_eq!(names(), ["us", "de", "fr", "gb"]);
        assert_eq!(us.iter().sum::<usize>(), (MAIN_BLOCK.len() - 1) * 2);
    }

    #[test]
    fn keys_are_translated_by_their_position() {
        let all = positions();
        let us = &all["us"];
        let typed = |to: &str, text: &str| -> String {
            text.chars().map(|c| us.translate(&all[to], c)).collect()
        };

        // The keys a German user presses for "Zürich-2" on a US console.
        assert_eq!(typed("de", "Y[rich/@"), "Zürich-\"");
        assert_eq!(typed("fr", "qzm2"), "aw,é");
        assert_eq!(typed("gb", "@\\"), "\"#");
        assert_eq!(all["de"].translate(us, 'z'), 'y');
        assert_eq!(us.translate(&all["de"], '€'), '€');
    }
}
