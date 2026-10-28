//! The Linux console's own sequences, blinking text, the reverse screen,
//! and the terminal's name.

use pretty_assertions::assert_eq;

use super::*;

const RED: Rgb = Rgb(0xaa, 0, 0);

#[test]
fn the_linux_palette_sequences_set_and_reset_colours() {
    let mut console = console();
    // No terminator: the text after them is text.
    console.feed(b"\x1b]P1123456\x1b[31mR\x1b[0m");
    console.draw();
    assert_eq!(console.text().lines().next(), Some("R"));
    assert_eq!(
        colours(&mut console, 0, 0),
        [Rgb::BLACK, Rgb(0x12, 0x34, 0x56)]
    );
    console.feed(b"\x1b]R");
    console.draw();
    assert_eq!(colours(&mut console, 0, 0), [Rgb::BLACK, RED]);
    // A colour that is not hexadecimal is left alone.
    console.feed(b"\x1b]P1zzzzzzx");
    console.draw();
    assert_eq!(colours(&mut console, 0, 0), [Rgb::BLACK, RED]);
    assert_eq!(console.text().lines().next(), Some("Rx"));
}

#[test]
fn the_linux_cursor_sequences_set_its_shape() {
    let mut console = console();
    console.feed(b"\x1b[?1c");
    assert!(console.drawn_cursor().is_none());
    for (sequence, shape) in [
        (&b"\x1b[?2c"[..], CursorShape::Underline),
        (b"\x1b[?6c", CursorShape::Block),
        (b"\x1b[?17;0;64c", CursorShape::Block),
        (b"\x1b[?0c", CursorShape::Block),
    ] {
        console.feed(sequence);
        assert_eq!(
            console.drawn_cursor().map(|cursor| cursor.shape),
            Some(shape)
        );
    }
    assert!(console.take_replies().is_empty());
}

#[test]
fn the_current_colours_become_the_default_ones() {
    let mut console = console();
    // Red on blue, made the default: reset attributes and erased cells
    // take them.
    console.feed(b"\x1b[31;44m\x1b[8]\x1b[1m\x1b[0mA\x1b[39mB\x1b[49m\x1b[K");
    console.feed(b"\x1b[9;30]\x1b[?25l");
    console.draw();
    let blue = Rgb(0, 0, 0xaa);
    assert_eq!(colours(&mut console, 0, 0), [blue, RED]);
    assert_eq!(colours(&mut console, 1, 0), [blue, RED]);
    assert_eq!(colours(&mut console, 5, 0), [blue]);
    // A reset forgets them.
    console.feed(b"\x1bc\x1b[?25lA");
    console.draw();
    assert_eq!(colours(&mut console, 0, 0), [Rgb::BLACK, GREY]);
}

#[test]
fn blinking_text_is_hidden_and_shown_while_on_the_screen() {
    let mut console = console();
    let start = Instant::now();
    console.feed(b"\x1b[?25l\x1b[5mB\x1b[25mS");
    // Nothing blinks before the console's first tick, the time it starts
    // from.
    console.draw();
    assert_eq!(console.deadline(), None);
    console.tick(start);
    console.draw();
    assert_eq!(console.deadline(), Some(start + BLINK));
    console.tick(start + BLINK);
    assert!(console.draw() > 0);
    assert_eq!(colours(&mut console, 0, 0), [Rgb::BLACK]);
    assert_eq!(colours(&mut console, 1, 0), [Rgb::BLACK, GREY]);
    console.tick(start + 2 * BLINK);
    console.draw();
    assert_eq!(colours(&mut console, 0, 0), [Rgb::BLACK, GREY]);
    // Once it is gone, nothing blinks.
    console.feed(b"\x1b[2J");
    console.draw();
    assert_eq!(console.deadline(), None);
}

#[test]
fn the_reverse_screen_swaps_the_colours_of_every_cell() {
    let mut console = console();
    console.feed(b"\x1b[?25l\x1b[31mR\x1b[0m");
    console.feed(b"\x1b[?5h\x1b[?5$p");
    assert_eq!(console.take_replies(), b"\x1b[?5;1$y");
    console.draw();
    assert_eq!(colours(&mut console, 0, 0), [Rgb::BLACK, RED]);
    assert_eq!(colours(&mut console, 1, 0), [GREY]);
    console.feed(b"\x1b[?5l");
    console.draw();
    assert_eq!(colours(&mut console, 1, 0), [Rgb::BLACK]);
}

#[test]
fn the_terminal_names_itself() {
    let mut console = console();
    console.feed(b"\x1b[>q");
    let version = zbm_platform::version::ZBM_VERSION;
    let expected = format!("\x1bP>|ZFSBootMenu({version})\x1b\\");
    assert_eq!(String::from_utf8(console.take_replies()).unwrap(), expected);
    console.feed(b"\x1b[>c");
    assert_eq!(console.take_replies(), b"\x1b[>0;30100;1c");
}
