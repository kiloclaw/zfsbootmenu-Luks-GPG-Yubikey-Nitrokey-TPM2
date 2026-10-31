use std::io::Write as _;

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::*;
use crate::menu::keymap::{Modifiers, press};

/// `code` with no modifier.
fn plain(code: Key) -> KeyPress {
    press(code, Modifiers::NONE)
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
    assert_eq!(keys.pending, [0x1b]);
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

#[test]
fn only_the_line_feed_right_after_a_carriage_return_joins_it() {
    // CR, then LF, then LF, each read alone: two Enters, CR LF and LF.
    let (mut keys, mut writer) = piped(CONSOLE_ESCAPE_WAIT);
    let enter = Some(plain(Key::Enter));
    writer.write_all(b"\r").unwrap();
    assert_eq!(keys.key(TICK).unwrap(), enter);
    writer.write_all(b"\n").unwrap();
    assert_eq!(keys.key(TICK).unwrap(), None);
    writer.write_all(b"\n").unwrap();
    assert_eq!(keys.key(TICK).unwrap(), enter);
}
