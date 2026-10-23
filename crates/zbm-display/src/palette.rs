//! The colours of the console: the Linux console's 16, and the 256 of
//! xterm that terminal programs and themes expect beyond them.

use crate::frame::Rgb;

/// The 16 colours of the Linux console, in the order of their numbers.
pub const CONSOLE: [Rgb; 16] = [
    Rgb(0x00, 0x00, 0x00),
    Rgb(0xaa, 0x00, 0x00),
    Rgb(0x00, 0xaa, 0x00),
    Rgb(0xaa, 0x55, 0x00),
    Rgb(0x00, 0x00, 0xaa),
    Rgb(0xaa, 0x00, 0xaa),
    Rgb(0x00, 0xaa, 0xaa),
    Rgb(0xaa, 0xaa, 0xaa),
    Rgb(0x55, 0x55, 0x55),
    Rgb(0xff, 0x55, 0x55),
    Rgb(0x55, 0xff, 0x55),
    Rgb(0xff, 0xff, 0x55),
    Rgb(0x55, 0x55, 0xff),
    Rgb(0xff, 0x55, 0xff),
    Rgb(0x55, 0xff, 0xff),
    Rgb(0xff, 0xff, 0xff),
];

/// The console's default text colour: grey.
pub const FOREGROUND: Rgb = CONSOLE[7];

/// The console's default background: black.
pub const BACKGROUND: Rgb = CONSOLE[0];

/// Colour `number` of the 256-colour palette: the console's 16, then a
/// 6×6×6 cube, then 24 greys.
pub fn indexed(number: u8) -> Rgb {
    if let Some(colour) = CONSOLE.get(usize::from(number)) {
        return *colour;
    }
    if number >= 232 {
        let grey = 8 + (number - 232) * 10;
        return Rgb(grey, grey, grey);
    }
    let cube = number - 16;
    let level = |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
    Rgb(level(cube / 36), level(cube / 6 % 6), level(cube % 6))
}

/// `colour` at half its brightness, as the console draws dim text.
pub fn dim(colour: Rgb) -> Rgb {
    Rgb(colour.0 / 2, colour.1 / 2, colour.2 / 2)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_256_colours_are_the_console_s_a_cube_and_greys() {
        assert_eq!(indexed(3), Rgb(0xaa, 0x55, 0x00));
        assert_eq!(indexed(15), Rgb(0xff, 0xff, 0xff));
        assert_eq!(indexed(16), Rgb(0, 0, 0));
        assert_eq!(indexed(196), Rgb(255, 0, 0));
        assert_eq!(indexed(231), Rgb(255, 255, 255));
        assert_eq!(indexed(232), Rgb(8, 8, 8));
        assert_eq!(indexed(255), Rgb(238, 238, 238));
        assert_eq!(dim(Rgb(0xff, 0x80, 1)), Rgb(0x7f, 0x40, 0));
        assert_eq!((FOREGROUND, BACKGROUND), (CONSOLE[7], Rgb::BLACK));
    }
}
