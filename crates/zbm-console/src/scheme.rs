//! The colour schemes of the console (`zbm.console_palette`): the 16 base
//! colours, the default text and background, and bold default text. The
//! 240 colours beyond the 16 are xterm's in every scheme.

use zbm_display::frame::Rgb;
use zbm_display::palette;

/// A colour scheme.
#[derive(Debug, Eq, Hash, PartialEq)]
pub struct Scheme {
    /// Its name, as `zbm.console_palette` gives it.
    pub name: &'static str,
    /// Colours 0 to 15.
    pub base: [Rgb; 16],
    /// Default text.
    pub foreground: Rgb,
    /// Default background.
    pub background: Rgb,
    /// Bold default text.
    pub bold: Rgb,
}

impl Scheme {
    /// Colour `number` of the 256: the scheme's 16, then xterm's.
    pub fn indexed(&self, number: u8) -> Rgb {
        self.base
            .get(usize::from(number))
            .copied()
            .unwrap_or_else(|| palette::indexed(number))
    }
}

/// The Linux console's colours: grey on black.
pub static LINUX: Scheme = Scheme {
    name: "linux",
    base: palette::CONSOLE,
    foreground: palette::CONSOLE[7],
    background: palette::CONSOLE[0],
    bold: palette::CONSOLE[15],
};

/// White on black, with every colour bright enough to read on black.
pub static HIGH_CONTRAST: Scheme = Scheme {
    name: "high-contrast",
    base: [
        Rgb(0x00, 0x00, 0x00),
        Rgb(0xff, 0x60, 0x60),
        Rgb(0x60, 0xff, 0x60),
        Rgb(0xff, 0xff, 0x40),
        Rgb(0x80, 0xa0, 0xff),
        Rgb(0xff, 0x70, 0xff),
        Rgb(0x40, 0xff, 0xff),
        Rgb(0xff, 0xff, 0xff),
        Rgb(0xc0, 0xc0, 0xc0),
        Rgb(0xff, 0x90, 0x90),
        Rgb(0x90, 0xff, 0x90),
        Rgb(0xff, 0xff, 0x90),
        Rgb(0xb0, 0xc8, 0xff),
        Rgb(0xff, 0xa0, 0xff),
        Rgb(0x90, 0xff, 0xff),
        Rgb(0xff, 0xff, 0xff),
    ],
    foreground: Rgb(0xff, 0xff, 0xff),
    background: Rgb(0x00, 0x00, 0x00),
    bold: Rgb(0xff, 0xff, 0x40),
};

/// Solarized's colours, `base03` to `base3` and its accents.
const SOLARIZED: [Rgb; 16] = [
    Rgb(0x07, 0x36, 0x42),
    Rgb(0xdc, 0x32, 0x2f),
    Rgb(0x85, 0x99, 0x00),
    Rgb(0xb5, 0x89, 0x00),
    Rgb(0x26, 0x8b, 0xd2),
    Rgb(0xd3, 0x36, 0x82),
    Rgb(0x2a, 0xa1, 0x98),
    Rgb(0xee, 0xe8, 0xd5),
    Rgb(0x00, 0x2b, 0x36),
    Rgb(0xcb, 0x4b, 0x16),
    Rgb(0x58, 0x6e, 0x75),
    Rgb(0x65, 0x7b, 0x83),
    Rgb(0x83, 0x94, 0x96),
    Rgb(0x6c, 0x71, 0xc4),
    Rgb(0x93, 0xa1, 0xa1),
    Rgb(0xfd, 0xf6, 0xe3),
];

/// Solarized on its dark background.
pub static SOLARIZED_DARK: Scheme = Scheme {
    name: "solarized-dark",
    base: SOLARIZED,
    foreground: SOLARIZED[12],
    background: SOLARIZED[8],
    bold: SOLARIZED[14],
};

/// Solarized on its light background.
pub static SOLARIZED_LIGHT: Scheme = Scheme {
    name: "solarized-light",
    base: SOLARIZED,
    foreground: SOLARIZED[11],
    background: SOLARIZED[15],
    bold: SOLARIZED[10],
};

/// The Tango desktop's colours, on black.
pub static TANGO: Scheme = Scheme {
    name: "tango",
    base: [
        Rgb(0x2e, 0x34, 0x36),
        Rgb(0xcc, 0x00, 0x00),
        Rgb(0x4e, 0x9a, 0x06),
        Rgb(0xc4, 0xa0, 0x00),
        Rgb(0x34, 0x65, 0xa4),
        Rgb(0x75, 0x50, 0x7b),
        Rgb(0x06, 0x98, 0x9a),
        Rgb(0xd3, 0xd7, 0xcf),
        Rgb(0x55, 0x57, 0x53),
        Rgb(0xef, 0x29, 0x29),
        Rgb(0x8a, 0xe2, 0x34),
        Rgb(0xfc, 0xe9, 0x4f),
        Rgb(0x72, 0x9f, 0xcf),
        Rgb(0xad, 0x7f, 0xa8),
        Rgb(0x34, 0xe2, 0xe2),
        Rgb(0xee, 0xee, 0xec),
    ],
    foreground: Rgb(0xd3, 0xd7, 0xcf),
    background: Rgb(0x00, 0x00, 0x00),
    bold: Rgb(0xee, 0xee, 0xec),
};

/// Every scheme, the default first.
pub static SCHEMES: [&Scheme; 5] = [
    &LINUX,
    &HIGH_CONTRAST,
    &SOLARIZED_DARK,
    &SOLARIZED_LIGHT,
    &TANGO,
];

/// The scheme named `name`.
pub fn by_name(name: &str) -> Option<&'static Scheme> {
    SCHEMES.iter().copied().find(|scheme| scheme.name == name)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn schemes_are_found_by_name_and_share_the_240_colours() {
        assert_eq!(by_name("linux"), Some(&LINUX));
        assert_eq!(by_name("tango").map(|scheme| scheme.name), Some("tango"));
        assert_eq!(by_name("rainbow"), None);
        for scheme in SCHEMES {
            assert_eq!(scheme.indexed(100), palette::indexed(100));
            assert_eq!(scheme.indexed(1), scheme.base[1]);
            assert_ne!(scheme.foreground, scheme.background, "{}", scheme.name);
        }
        assert_eq!(LINUX.indexed(7), palette::FOREGROUND);
    }

    #[test]
    fn high_contrast_text_stands_out_on_its_background() {
        // Relative luminance, roughly: every colour but black is light.
        let light = |Rgb(r, g, b): Rgb| u32::from(r) * 3 + u32::from(g) * 6 + u32::from(b) > 700;
        for colour in HIGH_CONTRAST.base.iter().skip(1) {
            assert!(light(*colour), "{colour:?}");
        }
    }
}
