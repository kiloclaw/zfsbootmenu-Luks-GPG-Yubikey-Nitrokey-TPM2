use std::io::Write as _;

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::*;
use crate::menu::keymap::{Modifiers, press};

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

/// Keys read from a pipe, the rest of whose sequences may take
/// `escape_wait`, and the pipe's other end.
fn piped(escape_wait: Duration) -> (TerminalKeys, io::PipeWriter) {
    let (reader, writer) = io::pipe().unwrap();
    let input = File::from(std::os::fd::OwnedFd::from(reader));
    (TerminalKeys::on(input, escape_wait), writer)
}

/// How long the tests wait for a key at once.
const TICK: Duration = Duration::from_millis(10);

/// The next key, asked for tick after tick for up to a second, and how long
/// it took to come.
#[allow(
    clippy::disallowed_methods,
    reason = "the reader waits on the wall clock, so the test measures it there"
)]
fn next_key(keys: &mut TerminalKeys) -> (Option<KeyPress>, Duration) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(1) {
        if let Some(key) = keys.key(TICK).unwrap() {
            return (Some(key), start.elapsed());
        }
    }
    (None, start.elapsed())
}

#[test]
fn keys_are_read_as_they_come_and_escape_alone_after_a_moment() {
    let (mut keys, mut writer) = piped(CONSOLE_ESCAPE_WAIT);
    let tick = TICK;
    assert_eq!(keys.key(tick).unwrap(), None);

    writer.write_all(b"pw\r\x1b").unwrap();
    assert_eq!(keys.key(tick).unwrap(), Some(plain(Key::Char('p'))));
    assert_eq!(keys.key(tick).unwrap(), Some(plain(Key::Char('w'))));
    assert_eq!(keys.key(tick).unwrap(), Some(plain(Key::Enter)));
    // Escape is held back while the rest of a sequence may come.
    assert_eq!(keys.pending, [ESCAPE]);
    assert_eq!(next_key(&mut keys).0, Some(plain(Key::Escape)));
    assert!(keys.pending.is_empty());

    writer.write_all(b"\x1b[").unwrap();
    assert_eq!(keys.key(tick).unwrap(), None);
    writer.write_all(b"B").unwrap();
    assert_eq!(keys.key(tick).unwrap(), Some(plain(Key::Down)));

    // What was not used yet is dropped when the terminal is handed over.
    writer.write_all(b"xy").unwrap();
    assert_eq!(keys.key(tick).unwrap(), Some(plain(Key::Char('x'))));
    keys.release().unwrap();
    assert_eq!(keys.key(tick).unwrap(), None);

    drop(writer);
    let ended = keys.key(tick).unwrap_err();
    assert_eq!(ended.kind(), io::ErrorKind::UnexpectedEof);
}

#[test]
fn escape_waits_for_the_rest_of_a_sequence_however_short_the_tick() {
    // Off the console, the rest of a sequence may come in another piece.
    let (mut keys, mut writer) = piped(Duration::from_secs(5));
    writer.write_all(b"\x1b").unwrap();
    assert_eq!(keys.key(TICK).unwrap(), None);
    assert_eq!(keys.key(TICK).unwrap(), None);
    writer.write_all(b"[A").unwrap();
    assert_eq!(keys.key(TICK).unwrap(), Some(plain(Key::Up)));

    // Escape alone comes once the wait is over, counted from its byte.
    for wait in [CONSOLE_ESCAPE_WAIT, SERIAL_ESCAPE_WAIT] {
        let (mut keys, mut writer) = piped(wait);
        writer.write_all(b"\x1b").unwrap();
        let (key, took) = next_key(&mut keys);
        assert_eq!(key, Some(plain(Key::Escape)));
        assert!(took >= wait, "Escape after {took:?}, before {wait:?}");
    }
    assert!(SERIAL_ESCAPE_WAIT > CONSOLE_ESCAPE_WAIT);
}

#[test]
fn the_keys_read_are_wiped_once_given() {
    let (mut keys, mut writer) = piped(CONSOLE_ESCAPE_WAIT);
    let tick = TICK;
    writer.write_all(b"secret").unwrap();
    for _ in 0..6 {
        assert!(keys.key(tick).unwrap().is_some());
    }
    assert_eq!(keys.keys.len(), 6);
    assert_eq!(keys.key(tick).unwrap(), None);
    assert!(keys.keys.is_empty() && keys.next == 0);
    assert!(keys.pending.is_empty());
}

#[test]
fn cr_lf_split_between_reads_is_one_enter() {
    let (mut keys, mut writer) = piped(CONSOLE_ESCAPE_WAIT);
    writer.write_all(b"pw\r").unwrap();
    for _ in 0..3 {
        assert!(keys.key(TICK).unwrap().is_some());
    }
    writer.write_all(b"\nx").unwrap();
    assert_eq!(keys.key(TICK).unwrap(), Some(plain(Key::Char('x'))));
    assert_eq!(keys.key(TICK).unwrap(), None);
    // The list of keys never grows past what it was made with.
    writer.write_all(&[b'a'; 3 * BUFFER]).unwrap();
    let capacity = keys.keys.capacity();
    while keys.key(TICK).unwrap().is_some() {}
    assert_eq!(keys.keys.capacity(), capacity);
}
