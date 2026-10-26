//! The font's size while the console runs.

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::*;
use crate::screen::zoom::{MIN_COLUMNS, Zoom};

fn ctrl_alt(c: char) -> KeyPress {
    KeyPress {
        key: Key::Char(c),
        ctrl: true,
        alt: true,
        ..KeyPress::BLANK
    }
}

/// Terminus 8×14, `scale` times as large.
fn terminus(scale: u32) -> Grid {
    Grid::new(Font::parse(TER_V14B).unwrap(), scale).named("ter-v14b.psf")
}

#[test]
fn ctrl_alt_keys_ask_for_a_size_once() {
    let mut console = console();
    for (c, zoom) in [
        ('=', Zoom::Bigger),
        ('+', Zoom::Bigger),
        ('-', Zoom::Smaller),
        ('0', Zoom::Start),
    ] {
        assert_eq!(console.take_key(ctrl_alt(c)), None);
        assert_eq!(console.take_zoom(), Some(zoom));
        assert_eq!(console.take_zoom(), None);
    }
    // Ctrl or Alt alone: the programs'.
    let ctrl = KeyPress {
        alt: false,
        ..ctrl_alt('=')
    };
    assert_eq!(console.take_key(ctrl), Some(ctrl));
    assert_eq!(console.take_key(ctrl_alt('x')), Some(ctrl_alt('x')));
    assert_eq!(console.take_zoom(), None);
}

#[test]
fn a_request_reads_a_step_the_first_size_or_a_height() {
    assert_eq!(Zoom::parse("+"), Some(Zoom::Bigger));
    assert_eq!(Zoom::parse("-"), Some(Zoom::Smaller));
    assert_eq!(Zoom::parse("default"), Some(Zoom::Start));
    assert_eq!(Zoom::parse("32"), Some(Zoom::Height(32)));
    for wrong in ["", "2", "1000", "big"] {
        assert_eq!(Zoom::parse(wrong), None, "{wrong}");
    }
}

#[test]
fn the_heights_tried_go_up_or_down_from_the_current_one() {
    assert_eq!(Zoom::Bigger.heights(28).first(), Some(&Some(32)));
    assert_eq!(Zoom::Smaller.heights(14), [Some(12), Some(10), Some(8)]);
    assert_eq!(Zoom::Start.heights(14), [None]);
    assert_eq!(Zoom::Height(20).heights(14), [Some(20)]);
    assert!(Zoom::Height(0).heights(14).is_empty());
    // A step up needs a larger cell and room enough.
    let pixels = (20 * 8, 5 * 14);
    assert!(!Zoom::Bigger.is_met_by(&terminus(1), pixels, 14));
    assert!(!Zoom::Bigger.is_met_by(&terminus(2), pixels, 14));
    let wide = (u32::from(MIN_COLUMNS) * 16, 10 * 28);
    assert!(Zoom::Bigger.is_met_by(&terminus(2), wide, 14));
    assert!(Zoom::Smaller.is_met_by(&terminus(1), pixels, 28));
    assert!(!Zoom::Smaller.is_met_by(&terminus(1), pixels, 14));
    assert!(Zoom::Height(28).is_met_by(&terminus(1), pixels, 14));
    assert!(!Zoom::Start.is_met_by(&terminus(8), pixels, 14));
}

#[test]
fn a_new_grid_gives_the_terminal_the_cells_that_fit_and_keeps_the_text() {
    let mut console = console().with_scrollback(20);
    console.feed(b"one\r\ntwo\r\nthree");
    console.draw();
    assert_eq!(console.font_text(), "psf 8x14, 20x5");
    console.set_grid(terminus(2));
    assert_eq!(console.size(), (10, 2));
    assert_eq!(console.window_size().cell_height, 28);
    assert_eq!(console.font_text(), "ter-v14b.psf 16x28, 10x2");
    assert!(console.draw() > 0);
    assert_eq!(console.text(), "two\nthree\n");
    // Back again: the lines that went into the history come back.
    console.set_grid(terminus(1));
    assert_eq!(console.size(), (20, 5));
    assert!(console.text().starts_with("one\ntwo\nthree\n"));
    assert!(console.take_replies().is_empty());
}

#[test]
fn a_colour_scheme_paints_the_frame_and_the_default_colours() {
    use crate::scheme::SOLARIZED_LIGHT;
    let mut console = console().with_scheme(&SOLARIZED_LIGHT);
    assert_eq!(console.scheme().name, "solarized-light");
    console.feed(b"x\x1b[1my");
    console.draw();
    let cell = |console: &mut Console, column| colours(console, column, 0);
    // The background, and the text in its default colour, then bold.
    assert_eq!(cell(&mut console, 5), [SOLARIZED_LIGHT.background]);
    assert!(cell(&mut console, 0).contains(&SOLARIZED_LIGHT.foreground));
    assert!(cell(&mut console, 1).contains(&SOLARIZED_LIGHT.bold));
    // A new grid paints the frame with it too.
    console.set_grid(terminus(2));
    console.draw();
    let corner = console.frame_mut().pixel(20 * 8 - 1, 5 * 14 - 1).unwrap();
    assert_eq!(corner, SOLARIZED_LIGHT.background);
}
