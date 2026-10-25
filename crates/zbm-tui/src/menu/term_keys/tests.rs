use std::io::Write as _;

use pretty_assertions::assert_eq;

use super::*;

fn with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

/// The keys `bytes` decode to, all of them having come.
fn keys(bytes: &[u8]) -> Vec<KeyEvent> {
    let mut keys = Vec::new();
    assert_eq!(decode(bytes, true, &mut keys), bytes.len());
    keys
}

#[test]
fn characters_and_control_characters_decode_as_the_terminal_library_does() {
    let shift = KeyModifiers::SHIFT;
    let ctrl = KeyModifiers::CONTROL;
    assert_eq!(
        keys("aZé€".as_bytes()),
        [
            plain(KeyCode::Char('a')),
            with(KeyCode::Char('Z'), shift),
            plain(KeyCode::Char('é')),
            plain(KeyCode::Char('€')),
        ]
    );
    assert_eq!(
        keys(b"\r\n\t\x7f\x08\x12\x0b\x15\x1c\0"),
        [
            plain(KeyCode::Enter),
            plain(KeyCode::Enter),
            plain(KeyCode::Tab),
            plain(KeyCode::Backspace),
            plain(KeyCode::Backspace),
            with(KeyCode::Char('r'), ctrl),
            with(KeyCode::Char('k'), ctrl),
            with(KeyCode::Char('u'), ctrl),
            with(KeyCode::Char('4'), ctrl),
            with(KeyCode::Char(' '), ctrl),
        ]
    );
    // Bytes that start no character are dropped.
    assert_eq!(keys(b"\xffa\x80"), [plain(KeyCode::Char('a'))]);
}

#[test]
fn the_sequences_of_the_linux_console_and_of_xterm_name_keys() {
    let sequences: [(&[u8], KeyCode); 14] = [
        (b"\x1b[A", KeyCode::Up),
        (b"\x1b[B", KeyCode::Down),
        (b"\x1b[C", KeyCode::Right),
        (b"\x1b[D", KeyCode::Left),
        (b"\x1bOA", KeyCode::Up),
        (b"\x1bOH", KeyCode::Home),
        (b"\x1b[F", KeyCode::End),
        (b"\x1b[1~", KeyCode::Home),
        (b"\x1b[3~", KeyCode::Delete),
        (b"\x1b[6~", KeyCode::PageDown),
        (b"\x1b[[A", KeyCode::F(1)),
        (b"\x1bOQ", KeyCode::F(2)),
        (b"\x1b[15~", KeyCode::F(5)),
        (b"\x1b[24~", KeyCode::F(12)),
    ];
    for (bytes, code) in sequences {
        assert_eq!(keys(bytes), [plain(code)], "{bytes:?}");
    }
    assert_eq!(
        keys(b"\x1b[1;5A\x1b[Z\x1bx\x1b\x1b"),
        [
            with(KeyCode::Up, KeyModifiers::CONTROL),
            with(KeyCode::BackTab, KeyModifiers::SHIFT),
            with(KeyCode::Char('x'), KeyModifiers::ALT),
            plain(KeyCode::Esc),
            plain(KeyCode::Esc),
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
    assert_eq!(keys, [plain(KeyCode::Char('a')), plain(KeyCode::Esc)]);
}

/// Keys read from a pipe, and its other end.
fn piped() -> (TerminalKeys, io::PipeWriter) {
    let (reader, writer) = io::pipe().unwrap();
    let input = File::from(std::os::fd::OwnedFd::from(reader));
    (TerminalKeys::on(input), writer)
}

#[test]
fn keys_are_read_as_they_come_and_escape_alone_after_a_moment() {
    let (mut keys, mut writer) = piped();
    let tick = Duration::from_millis(10);
    assert_eq!(keys.key(tick).unwrap(), None);

    writer.write_all(b"pw\r\x1b").unwrap();
    assert_eq!(keys.key(tick).unwrap(), Some(plain(KeyCode::Char('p'))));
    assert_eq!(keys.key(tick).unwrap(), Some(plain(KeyCode::Char('w'))));
    assert_eq!(keys.key(tick).unwrap(), Some(plain(KeyCode::Enter)));
    // Escape is held back while the rest of a sequence may come.
    assert_eq!(keys.pending, [ESCAPE]);
    assert_eq!(keys.key(tick).unwrap(), Some(plain(KeyCode::Esc)));
    assert!(keys.pending.is_empty());

    writer.write_all(b"\x1b[").unwrap();
    assert_eq!(keys.key(tick).unwrap(), None);
    writer.write_all(b"B").unwrap();
    assert_eq!(keys.key(tick).unwrap(), Some(plain(KeyCode::Down)));

    // What was not used yet is dropped when the terminal is handed over.
    writer.write_all(b"xy").unwrap();
    assert_eq!(keys.key(tick).unwrap(), Some(plain(KeyCode::Char('x'))));
    keys.release().unwrap();
    assert_eq!(keys.key(tick).unwrap(), None);

    drop(writer);
    let ended = keys.key(tick).unwrap_err();
    assert_eq!(ended.kind(), io::ErrorKind::UnexpectedEof);
}

#[test]
fn the_keys_read_are_wiped_once_given() {
    let (mut keys, mut writer) = piped();
    let tick = Duration::from_millis(10);
    writer.write_all(b"secret").unwrap();
    for _ in 0..6 {
        assert!(keys.key(tick).unwrap().is_some());
    }
    assert_eq!(keys.keys.len(), 6);
    assert_eq!(keys.key(tick).unwrap(), None);
    assert!(keys.keys.is_empty() && keys.next == 0);
    assert!(keys.pending.is_empty());
}
