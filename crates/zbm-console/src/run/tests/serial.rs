//! The serial console too (ROADMAP.md UI9): the session has the size of
//! the terminal typed on last.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn keys_from_the_serial_console_hold_the_screen_to_its_size_until_the_keyboard_types() {
    let mut screen = FakeScreen {
        presented: 0,
        broken: false,
    };
    let mut console = console();
    let mut host = FakeHost::new(vec![
        Input::SerialKeys(vec![press(Key::Char('a'))]),
        Input::SerialKeys(vec![press(Key::Char('b'))]),
        Input::Keys(vec![press(Key::Char('c'))]),
    ]);
    host.serial = Some((20, 5));

    run(&mut console, &mut screen, &mut host, None).unwrap();

    // Told once on the serial console's first key, once back at the desk.
    let sizes: Vec<(u16, u16)> = host
        .fonts_shown
        .iter()
        .map(|(_, columns, rows)| (*columns, *rows))
        .collect();
    assert_eq!(sizes, [(20, 5), (40, 10)]);
    assert_eq!(host.sent, b"abc");
    assert_eq!(console.held_size(), None);
}

#[test]
fn a_host_without_a_serial_console_fills_the_display() {
    let mut screen = FakeScreen {
        presented: 0,
        broken: false,
    };
    let mut console = console();
    let mut host = FakeHost::new(vec![Input::SerialKeys(vec![press(Key::Char('a'))])]);

    run(&mut console, &mut screen, &mut host, None).unwrap();

    assert!(host.fonts_shown.is_empty());
    assert_eq!(console.size(), (40, 10));
    assert_eq!(Bare.serial_size(), None);
}
