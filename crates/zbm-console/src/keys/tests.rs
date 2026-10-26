use pretty_assertions::assert_eq;

use super::*;
use zbm_display::keys::Media;

const LINUX: Modes = Modes {
    flavour: Flavour::Linux,
    application_cursor: false,
    application_keypad: false,
    new_line: false,
    kitty: 0,
};

const XTERM: Modes = Modes {
    flavour: Flavour::Xterm,
    ..LINUX
};

fn press(key: Key, shift: bool, ctrl: bool, alt: bool) -> KeyPress {
    KeyPress {
        key,
        shift,
        ctrl,
        alt,
        logo: false,
        keypad: false,
    }
}

fn bytes(press: KeyPress, modes: Modes) -> Vec<u8> {
    let mut out = Vec::new();
    encode(press, modes, &mut out);
    out
}

fn plain(key: Key) -> Vec<u8> {
    bytes(press(key, false, false, false), LINUX)
}

fn ctrl(key: Key) -> KeyPress {
    press(key, false, true, false)
}

fn keypad(key: Key) -> KeyPress {
    KeyPress {
        logo: false,
        keypad: true,
        ..press(key, false, false, false)
    }
}

#[test]
fn characters_and_their_controls() {
    assert_eq!(plain(Key::Char('a')), b"a");
    assert_eq!(plain(Key::Char('é')), "é".as_bytes());
    assert_eq!(bytes(ctrl(Key::Char('c')), LINUX), [3]);
    assert_eq!(bytes(press(Key::Char('D'), true, true, false), XTERM), [4]);
    assert_eq!(bytes(ctrl(Key::Char('[')), LINUX), [0x1b]);
    assert_eq!(bytes(ctrl(Key::Char(' ')), LINUX), [0]);
    assert_eq!(
        bytes(press(Key::Char('?'), true, true, false), LINUX),
        [0x7f]
    );
    assert_eq!(bytes(ctrl(Key::Char('1')), LINUX), b"1");
    assert_eq!(
        bytes(press(Key::Char('x'), false, false, true), XTERM),
        b"\x1bx"
    );
    assert_eq!(
        bytes(press(Key::Char('k'), false, true, true), LINUX),
        [0x1b, 11]
    );
    let others: Vec<Option<u8>> = ['\\', ']', '^', '_', '/', '2', '8']
        .into_iter()
        .map(control)
        .collect();
    assert_eq!(
        others,
        [
            Some(0x1c),
            Some(0x1d),
            Some(0x1e),
            Some(0x1f),
            Some(0x1f),
            Some(0),
            Some(0x7f)
        ]
    );
}

#[test]
fn keys_that_type_nothing() {
    assert_eq!(plain(Key::Enter), b"\r");
    assert_eq!(plain(Key::Escape), b"\x1b");
    assert_eq!(plain(Key::Backspace), b"\x7f");
    assert_eq!(bytes(ctrl(Key::Backspace), XTERM), [8]);
    assert_eq!(plain(Key::Tab), b"\t");
    assert_eq!(
        bytes(press(Key::BackTab, true, false, false), XTERM),
        b"\x1b[Z"
    );
    let alt = |key| bytes(press(key, false, false, true), LINUX);
    assert_eq!(alt(Key::Enter), b"\x1b\r");
    assert_eq!(alt(Key::Escape), b"\x1b\x1b");
    assert_eq!(alt(Key::Tab), b"\x1b\t");
    assert_eq!(alt(Key::Backspace), b"\x1b\x7f");
}

#[test]
fn the_linux_console_s_sequences_ignore_modifiers() {
    let application = Modes {
        application_cursor: true,
        ..LINUX
    };
    assert_eq!(plain(Key::Up), b"\x1b[A");
    assert_eq!(
        bytes(press(Key::Up, false, false, false), application),
        b"\x1bOA"
    );
    assert_eq!(bytes(ctrl(Key::Left), application), b"\x1bOD");
    assert_eq!(
        bytes(press(Key::Right, true, false, true), LINUX),
        b"\x1b[C"
    );
    assert_eq!(plain(Key::F(1)), b"\x1b[[A");
    assert_eq!(plain(Key::F(5)), b"\x1b[[E");
    assert_eq!(
        bytes(press(Key::F(2), true, false, false), LINUX),
        b"\x1b[[B"
    );
    assert_eq!(plain(Key::F(6)), b"\x1b[17~");
    assert_eq!(plain(Key::F(10)), b"\x1b[21~");
    assert_eq!(plain(Key::F(11)), b"\x1b[23~");
    assert_eq!(plain(Key::F(12)), b"\x1b[24~");
    assert_eq!(plain(Key::Home), b"\x1b[1~");
    assert_eq!(plain(Key::Insert), b"\x1b[2~");
    assert_eq!(plain(Key::Delete), b"\x1b[3~");
    assert_eq!(plain(Key::End), b"\x1b[4~");
    assert_eq!(plain(Key::PageUp), b"\x1b[5~");
    assert_eq!(bytes(ctrl(Key::PageDown), LINUX), b"\x1b[6~");
    // F13 to F20 as the Linux console's terminal description has them;
    // there is no F21, nor a sequence for Print or the media keys.
    assert_eq!(plain(Key::F(13)), b"\x1b[25~");
    assert_eq!(plain(Key::F(15)), b"\x1b[28~");
    assert_eq!(plain(Key::F(17)), b"\x1b[31~");
    assert_eq!(plain(Key::F(20)), b"\x1b[34~");
    assert_eq!(plain(Key::F(21)), b"");
    assert_eq!(plain(Key::Print), b"");
    assert_eq!(plain(Key::Media(Media::Mute)), b"");
}

#[test]
fn xterm_s_f13_to_f24_are_f1_to_f12_with_shift_and_the_logo_key_is_meta() {
    let xterm = |press| bytes(press, XTERM);
    assert_eq!(xterm(press(Key::F(13), false, false, false)), b"\x1b[1;2P");
    assert_eq!(xterm(press(Key::F(24), false, false, false)), b"\x1b[24;2~");
    let logo = KeyPress {
        logo: true,
        ..press(Key::Left, false, false, false)
    };
    assert_eq!(xterm(logo), b"\x1b[1;9D");
}

#[test]
fn xterm_s_sequences_carry_the_modifiers() {
    let xterm = |press| bytes(press, XTERM);
    let application = Modes {
        application_cursor: true,
        ..XTERM
    };
    assert_eq!(xterm(press(Key::Up, false, false, false)), b"\x1b[A");
    assert_eq!(
        bytes(press(Key::Up, false, false, false), application),
        b"\x1bOA"
    );
    assert_eq!(xterm(ctrl(Key::Left)), b"\x1b[1;5D");
    assert_eq!(bytes(ctrl(Key::Left), application), b"\x1b[1;5D");
    assert_eq!(xterm(press(Key::Right, true, false, true)), b"\x1b[1;4C");
    assert_eq!(xterm(press(Key::Home, false, false, false)), b"\x1b[H");
    assert_eq!(
        bytes(press(Key::End, false, false, false), application),
        b"\x1bOF"
    );
    assert_eq!(xterm(press(Key::End, true, false, false)), b"\x1b[1;2F");
    assert_eq!(xterm(press(Key::F(1), false, false, false)), b"\x1bOP");
    assert_eq!(xterm(press(Key::F(4), false, true, false)), b"\x1b[1;5S");
    assert_eq!(xterm(press(Key::F(5), false, false, false)), b"\x1b[15~");
    assert_eq!(xterm(press(Key::F(12), true, false, false)), b"\x1b[24;2~");
    assert_eq!(xterm(press(Key::Delete, false, true, true)), b"\x1b[3;7~");
    assert_eq!(xterm(press(Key::PageUp, false, false, false)), b"\x1b[5~");
}

#[test]
fn the_keypad_in_application_mode_sends_its_own_sequences() {
    let xterm = Modes {
        application_keypad: true,
        ..XTERM
    };
    let linux = Modes {
        flavour: Flavour::Linux,
        ..xterm
    };
    assert_eq!(bytes(keypad(Key::Char('0')), xterm), b"\x1bOp");
    assert_eq!(bytes(keypad(Key::Char('9')), linux), b"\x1bOy");
    assert_eq!(bytes(keypad(Key::Enter), xterm), b"\x1bOM");
    assert_eq!(bytes(keypad(Key::Char('+')), xterm), b"\x1bOk");
    assert_eq!(bytes(keypad(Key::Char('+')), linux), b"\x1bOl");
    for (c, ours, theirs) in [('-', b'm', b'S'), ('*', b'j', b'R'), ('/', b'o', b'Q')] {
        assert_eq!(bytes(keypad(Key::Char(c)), xterm), [0x1b, b'O', ours]);
        assert_eq!(bytes(keypad(Key::Char(c)), linux), [0x1b, b'O', theirs]);
    }
    assert_eq!(bytes(keypad(Key::Char('.')), xterm), b"\x1bOn");
    // Without Num Lock the keypad moves, as the cursor keys do.
    assert_eq!(bytes(keypad(Key::Left), xterm), b"\x1b[D");
    // In the normal mode it types, and keys off the keypad always do.
    assert_eq!(bytes(keypad(Key::Char('7')), XTERM), b"7");
    assert_eq!(
        bytes(press(Key::Char('7'), false, false, false), xterm),
        b"7"
    );
}

#[test]
fn enter_sends_a_line_feed_too_in_new_line_mode() {
    let new_line = Modes {
        new_line: true,
        ..XTERM
    };
    assert_eq!(
        bytes(press(Key::Enter, false, false, false), new_line),
        b"\r\n"
    );
    assert_eq!(
        bytes(press(Key::Enter, false, false, true), new_line),
        b"\x1b\r\n"
    );
}

#[test]
fn the_kitty_protocol_tells_ambiguous_keys_apart() {
    let kitty = Modes {
        kitty: DISAMBIGUATE,
        ..XTERM
    };
    let all = Modes {
        kitty: DISAMBIGUATE | ALL_KEYS,
        ..XTERM
    };
    let text = Modes {
        kitty: ALL_KEYS | TEXT,
        ..XTERM
    };
    assert_eq!(
        bytes(press(Key::Escape, false, false, false), kitty),
        b"\x1b[27u"
    );
    assert_eq!(bytes(ctrl(Key::Char('i')), kitty), b"\x1b[105;5u");
    assert_eq!(
        bytes(press(Key::Char('A'), true, true, false), kitty),
        b"\x1b[97;6u"
    );
    assert_eq!(
        bytes(press(Key::Char('x'), false, false, true), kitty),
        b"\x1b[120;3u"
    );
    // Typed text, Enter, Tab and Backspace stay as they are.
    assert_eq!(
        bytes(press(Key::Char('A'), true, false, false), kitty),
        b"A"
    );
    assert_eq!(bytes(press(Key::Enter, false, false, false), kitty), b"\r");
    assert_eq!(bytes(ctrl(Key::Enter), kitty), b"\x1b[13;5u");
    assert_eq!(
        bytes(press(Key::Tab, true, false, false), kitty),
        b"\x1b[9;2u"
    );
    // Keys with sequences of their own keep them.
    assert_eq!(bytes(ctrl(Key::Left), kitty), b"\x1b[1;5D");
    // Every key, and the text it types.
    assert_eq!(
        bytes(press(Key::Char('a'), false, false, false), all),
        b"\x1b[97u"
    );
    assert_eq!(
        bytes(press(Key::Enter, false, false, false), all),
        b"\x1b[13u"
    );
    assert_eq!(
        bytes(press(Key::Backspace, false, false, false), all),
        b"\x1b[127u"
    );
    assert_eq!(
        bytes(press(Key::BackTab, true, false, false), all),
        b"\x1b[9;2u"
    );
    assert_eq!(
        bytes(press(Key::Char('é'), false, false, false), text),
        b"\x1b[233;1;233u"
    );
    assert_eq!(
        bytes(press(Key::Char('B'), true, false, false), text),
        b"\x1b[98;2;66u"
    );
    assert_eq!(bytes(ctrl(Key::Char('c')), text), b"\x1b[99;5u");
}

#[test]
fn the_terminal_descriptions_have_their_names() {
    assert_eq!(Flavour::default().term(), "linux");
    assert_eq!(Flavour::Xterm.term(), "xterm-256color");
}

#[test]
fn images_with_xterm_s_description_get_its_keys() {
    let has =
        |wanted: &'static str| move |path: &std::path::Path| path == std::path::Path::new(wanted);
    assert_eq!(
        Flavour::of_image(has("/lib/terminfo/x/xterm-256color")),
        Flavour::Xterm
    );
    assert_eq!(
        Flavour::of_image(has("/usr/share/terminfo/x/xterm-256color")),
        Flavour::Xterm
    );
    assert_eq!(
        Flavour::of_image(has("/usr/share/terminfo/l/linux")),
        Flavour::Linux
    );
}

#[test]
fn the_kitty_protocol_names_keys_without_sequences_and_the_logo_key() {
    let kitty = Modes {
        kitty: DISAMBIGUATE,
        ..XTERM
    };
    let plain = |key| bytes(press(key, false, false, false), kitty);
    assert_eq!(plain(Key::Print), b"\x1b[57361u");
    assert_eq!(plain(Key::Pause), b"\x1b[57362u");
    assert_eq!(plain(Key::Menu), b"\x1b[57363u");
    assert_eq!(plain(Key::F(13)), b"\x1b[57376u");
    assert_eq!(plain(Key::F(24)), b"\x1b[57387u");
    let media = [
        (Media::PlayPause, 57430),
        (Media::Stop, 57432),
        (Media::Next, 57435),
        (Media::Previous, 57436),
        (Media::VolumeDown, 57438),
        (Media::VolumeUp, 57439),
        (Media::Mute, 57440),
    ];
    for (key, code) in media {
        assert_eq!(plain(Key::Media(key)), format!("\x1b[{code}u").as_bytes());
    }
    // A character with the logo key held is a key, not text.
    let logo = KeyPress {
        logo: true,
        ..press(Key::Char('l'), false, false, false)
    };
    assert_eq!(bytes(logo, kitty), b"\x1b[108;9u");
}
