//! Keyboard layouts for what is typed (`docs/dev/menu-design/unlocking.md`):
//! the console turns key positions into characters with its own layout, and
//! the menu turns them into the characters of the layout the user chose, so
//! a passphrase set on a German or French keyboard can be typed on one.

/// The console's own layout when the image names none.
pub const CONSOLE: &str = "us";

/// The rows of each layout the menu knows, unshifted and shifted, in the
/// order of the keys: the same index is the same key in every layout. Only
/// the keys of the main block are listed; the others pass unchanged.
const LAYOUTS: &[(&str, [&str; 8])] = &[
    (
        CONSOLE,
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
        "uk",
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

/// The layouts the menu can translate between.
pub fn known() -> impl Iterator<Item = &'static str> {
    LAYOUTS.iter().map(|(name, _)| *name)
}

/// The rows of `layout`.
fn rows(layout: &str) -> Option<&'static [&'static str; 8]> {
    LAYOUTS
        .iter()
        .find(|(name, _)| *name == layout)
        .map(|(_, rows)| rows)
}

/// The character the key that gives `c` in the layout `from` gives in the
/// layout `to`; `c` itself when either layout is unknown or the key is not
/// in the main block.
pub fn translate(from: &str, to: &str, c: char) -> char {
    if from == to {
        return c;
    }
    let (Some(source), Some(target)) = (rows(from), rows(to)) else {
        return c;
    };
    source
        .iter()
        .zip(target)
        .find_map(|(source, target)| {
            let index = source.chars().position(|known| known == c)?;
            target.chars().nth(index)
        })
        .unwrap_or(c)
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
        assert_eq!(known().collect::<Vec<_>>(), ["us", "de", "fr", "uk"]);
    }

    #[test]
    fn keys_are_translated_by_their_position() {
        let typed =
            |to, text: &str| -> String { text.chars().map(|c| translate("us", to, c)).collect() };

        // The keys a German user presses for "Zürich-2" on a US console.
        assert_eq!(typed("de", "Y[rich/@"), "Zürich-\"");
        assert_eq!(typed("fr", "qzm2"), "aw,é");
        assert_eq!(typed("uk", "@\\"), "\"#");
        assert_eq!(translate("de", "us", 'z'), 'y');
        assert_eq!(translate("us", "us", 'y'), 'y');
        assert_eq!(translate("us", "xx", 'y'), 'y');
        assert_eq!(translate("us", "de", '€'), '€');
    }
}
