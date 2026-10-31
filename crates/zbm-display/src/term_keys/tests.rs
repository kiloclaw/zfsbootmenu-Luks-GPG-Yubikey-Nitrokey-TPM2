use pretty_assertions::assert_eq;

use super::*;

fn with(code: Key, modifiers: Modifiers) -> KeyPress {
    press(code, modifiers)
}

/// The keys `bytes` decode to, all of them having come.
fn keys(bytes: &[u8]) -> Vec<KeyPress> {
    let mut keys = Vec::new();
    assert_eq!(decode(bytes, true, &mut keys), bytes.len());
    keys
}

#[test]
fn characters_and_control_characters_decode_as_the_terminal_library_does() {
    let shift = Modifiers::SHIFT;
    let ctrl = Modifiers::CTRL;
    assert_eq!(
        keys("aZé€".as_bytes()),
        [
            plain(Key::Char('a')),
            with(Key::Char('Z'), shift),
            plain(Key::Char('é')),
            plain(Key::Char('€')),
        ]
    );
    // CR LF is one Enter; CR or LF alone is one too.
    assert_eq!(
        keys(b"\r\n\n\r\t\x7f\x08\x12\x0b\x15\x1c\0"),
        [
            plain(Key::Enter),
            plain(Key::Enter),
            plain(Key::Enter),
            plain(Key::Tab),
            plain(Key::Backspace),
            plain(Key::Backspace),
            with(Key::Char('r'), ctrl),
            with(Key::Char('k'), ctrl),
            with(Key::Char('u'), ctrl),
            with(Key::Char('4'), ctrl),
            with(Key::Char(' '), ctrl),
        ]
    );
    // Bytes that start no character are dropped.
    assert_eq!(keys(b"\xffa\x80"), [plain(Key::Char('a'))]);
}

#[test]
fn the_sequences_of_the_linux_console_and_of_xterm_name_keys() {
    let sequences: [(&[u8], Key); 14] = [
        (b"\x1b[A", Key::Up),
        (b"\x1b[B", Key::Down),
        (b"\x1b[C", Key::Right),
        (b"\x1b[D", Key::Left),
        (b"\x1bOA", Key::Up),
        (b"\x1bOH", Key::Home),
        (b"\x1b[F", Key::End),
        (b"\x1b[1~", Key::Home),
        (b"\x1b[3~", Key::Delete),
        (b"\x1b[6~", Key::PageDown),
        (b"\x1b[[A", Key::F(1)),
        (b"\x1bOQ", Key::F(2)),
        (b"\x1b[15~", Key::F(5)),
        (b"\x1b[24~", Key::F(12)),
    ];
    for (bytes, code) in sequences {
        assert_eq!(keys(bytes), [plain(code)], "{bytes:?}");
    }
    assert_eq!(
        keys(b"\x1b[1;5A\x1b[Z\x1bx\x1b\x1b"),
        [
            with(Key::Up, Modifiers::CTRL),
            with(Key::BackTab, Modifiers::SHIFT),
            with(Key::Char('x'), Modifiers::ALT),
            plain(Key::Escape),
            plain(Key::Escape),
        ]
    );
    // Sequences of keys the menu has no use for are dropped.
    assert_eq!(keys(b"\x1b[99~\x1b[5n\x1bOz\x1b[[Z"), []);
}

#[test]
fn a_sequence_that_is_not_whole_waits_for_the_rest_or_is_the_escape_key() {
    let mut keys = Vec::new();
    for unfinished in [
        &b"\x1b"[..],
        b"\x1b[",
        b"\x1b[1;",
        b"\x1bO",
        b"\x1b[[",
        b"\xc3",
    ] {
        assert_eq!(decode(unfinished, false, &mut keys), 0, "{unfinished:?}");
    }
    assert!(keys.is_empty());
    assert_eq!(decode(b"a\x1b[1", false, &mut keys), 1);
    assert_eq!(decode(b"\x1b[1", true, &mut keys), 3);
    assert_eq!(decode(b"\xc3", true, &mut keys), 1);
    assert_eq!(keys, [plain(Key::Char('a')), plain(Key::Escape)]);
}

#[test]
fn a_terminal_s_replies_type_nothing() {
    // Its colours (OSC 11, ended by BEL and by ST), a DCS reply, and a
    // cursor position report: the keys around them still come.
    let replies: [&[u8]; 4] = [
        b"\x1b]11;rgb:0000/0000/0000\x07",
        b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\",
        b"\x1bP1$r0m\x1b\\",
        b"\x1b[12;40R",
    ];
    for reply in replies {
        let mut bytes = b"a".to_vec();
        bytes.extend_from_slice(reply);
        bytes.push(b'b');
        assert_eq!(
            keys(&bytes),
            [
                with(Key::Char('a'), Modifiers::NONE),
                with(Key::Char('b'), Modifiers::NONE)
            ],
            "{reply:?}"
        );
    }
}

#[test]
fn alt_with_a_bracket_is_still_typed() {
    // Without the end of a reply when nothing more comes, it is Alt+].
    assert_eq!(keys(b"\x1b]"), [with(Key::Char(']'), Modifiers::ALT)]);
    // While more may come, it waits.
    let mut held = Vec::new();
    assert_eq!(decode(b"\x1b]11;rgb:00", false, &mut held), 0);
    assert!(held.is_empty());
    // A reply without an end is not waited for without limit.
    let mut endless = b"\x1b]".to_vec();
    endless.resize(STRING_MAX, b'x');
    assert_eq!(decode(&endless, false, &mut held), STRING_MAX);
    assert!(held.is_empty());
}

#[test]
fn c1_controls_type_nothing() {
    assert_eq!(keys("x\u{9b}y".as_bytes()).len(), 2);
}

#[test]
fn a_broken_character_does_not_swallow_the_key_after_it() {
    // A stray lead byte, then Enter: the Enter still comes, read at once or
    // cut between two reads.
    assert_eq!(keys(b"\xdd\r"), [with(Key::Enter, Modifiers::NONE)]);
    assert_eq!(keys(b"\xe2\x82a"), [with(Key::Char('a'), Modifiers::NONE)]);
    let mut held = Vec::new();
    assert_eq!(decode(b"\xdd", false, &mut held), 0);
    assert_eq!(decode(b"\xdd", true, &mut held), 1);
    assert!(held.is_empty());
}

#[test]
fn a_broken_sequence_does_not_swallow_the_key_after_it() {
    // `ESC O` or `ESC [` cut short by Enter: the Enter still comes.
    assert_eq!(
        keys(b"\x1bO\r"),
        [
            with(Key::Char('O'), Modifiers::ALT | Modifiers::SHIFT),
            with(Key::Enter, Modifiers::NONE)
        ]
    );
    assert_eq!(keys(b"\x1b[1;5\r"), [with(Key::Enter, Modifiers::NONE)]);
    assert_eq!(keys(b"\x1b[[\r"), [with(Key::Enter, Modifiers::NONE)]);
    // An Enter that a carriage return ended joins a line feed after it,
    // with Alt too; a carriage return inside a broken sequence does not.
    let enter = [with(Key::Enter, Modifiers::NONE)];
    assert!(ends_with_carriage_return(b"\r", &enter));
    assert!(ends_with_carriage_return(
        b"\x1b\r",
        &[with(Key::Enter, Modifiers::ALT)]
    ));
    assert!(!ends_with_carriage_return(b"\x1b[1\r", &[]));
}
