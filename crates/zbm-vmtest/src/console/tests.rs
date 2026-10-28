use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use pretty_assertions::assert_eq;

use super::*;

/// A log that the test can read.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl Write for Log {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A console and the virtual machine's end of it.
fn console() -> (Console, UnixStream, Log) {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let log = Log::default();
    let reader = ours.try_clone().unwrap();
    let console = Console::attach(reader, ours, log.clone(), Size::default());
    (console, theirs, log)
}

const SECOND: Duration = Duration::from_secs(1);

#[test]
fn expect_returns_the_pattern_that_matched_with_what_came_before() {
    let (mut console, mut vm, log) = console();

    vm.write_all(b"zbm# \x1b[1mls\x1b[0m\r\nRC=0\r\n").unwrap();
    let index = console
        .expect(&["nothing", r"RC=(\d+)\r*\n"], SECOND)
        .unwrap();

    assert_eq!(index, 1);
    assert_eq!(console.group(1), "0");
    assert_eq!(console.before(), "zbm# ls\n");
    assert_eq!(log.0.lock().unwrap().len(), 23);
    assert_eq!(console.bytes_read(), 23);
}

#[test]
fn a_match_consumes_the_text_up_to_its_end() {
    let (mut console, mut vm, _log) = console();

    vm.write_all(b"one two one").unwrap();
    console.expect(&["one"], SECOND).unwrap();
    console.expect(&["one"], SECOND).unwrap();

    assert_eq!(console.before(), " two ");
}

#[test]
fn text_that_arrives_while_waiting_is_matched() {
    let (mut console, mut vm, _log) = console();
    #[allow(
        clippy::disallowed_methods,
        reason = "a developer tool, not the runtime of ADR 0001"
    )]
    let writer = std::thread::spawn(move || {
        for chunk in [&b"Passphrase (leave "[..], b"empty to ", b"skip): "] {
            std::thread::sleep(Duration::from_millis(30));
            vm.write_all(chunk).unwrap();
        }
        vm
    });

    let index = console.expect(&[r"Passphrase \(leave empty to skip\): "], SECOND * 5);

    assert_eq!(index.unwrap(), 0);
    drop(writer.join());
}

#[test]
fn a_timeout_names_the_patterns_and_what_the_console_showed() {
    let (mut console, mut vm, _log) = console();

    vm.write_all(b"Unable to import").unwrap();
    let error = console
        .expect(&["RC="], Duration::from_millis(100))
        .unwrap_err();

    assert!(matches!(&error, Error::Timeout { tail, .. } if tail == "Unable to import"));
    assert!(error.to_string().contains("RC="));
}

#[test]
fn a_closed_console_ends_the_wait() {
    let (mut console, vm, _log) = console();

    drop(vm);
    let error = console.expect(&["RC="], SECOND * 5).unwrap_err();

    assert!(matches!(error, Error::Eof { .. }));
    assert!(console.expect_eof(SECOND).is_ok());
}

#[test]
fn expect_eof_waits_for_the_end_and_keeps_the_rest() {
    let (mut console, mut vm, _log) = console();

    vm.write_all(b"sysrq: Power Off\r\n").unwrap();
    assert!(console.expect_eof(Duration::from_millis(50)).is_err());
    drop(vm);

    assert!(console.expect_eof(SECOND).is_ok());
    assert_eq!(console.before(), "sysrq: Power Off\n");
}

#[test]
fn an_invalid_pattern_is_an_error() {
    let (mut console, _vm, _log) = console();

    let error = console.expect(&["("], SECOND).unwrap_err();

    assert!(matches!(error, Error::Pattern { .. }));
}

#[test]
fn the_screen_shows_text_drawn_with_cursor_movements() {
    let (mut console, mut vm, _log) = console();

    vm.write_all(b"\x1b[2J\x1b[1;1HBoot\x1b[1;6Henvironments\x1b[3;1Hdone")
        .unwrap();
    console.expect(&["done"], SECOND).unwrap();

    let screen = console.screen();
    assert_eq!(screen.lines().next(), Some("Boot environments"));
    assert_eq!(screen.lines().nth(2), Some("done"));
}

/// A frame of `cols` columns on 25 lines: a title at the top left, a word
/// at its right edge, a key bar at the bottom.
fn frame(cols: u16) -> Vec<u8> {
    let right = cols - 2;
    format!("\x1b[2J\x1b[1;1HBOOT ENVIRONMENTS\x1b[1;{right}Hend\x1b[25;1HEnter Boot").into_bytes()
}

#[test]
fn the_screen_has_the_lines_of_a_serial_console_after_pre_init() {
    let (mut console, mut vm, _log) = console();

    vm.write_all(&frame(80)).unwrap();
    console.expect(&["Enter Boot"], SECOND).unwrap();

    let screen = console.screen();
    let lines: Vec<&str> = screen.lines().collect();
    assert_eq!(
        lines.first().copied(),
        Some(format!("{:<77}end", "BOOT ENVIRONMENTS").as_str())
    );
    assert_eq!(lines.get(24).copied(), Some("Enter Boot"));
}

#[test]
fn a_frame_as_wide_as_the_shell_s_terminal_lands_whole_once_the_screen_is() {
    let (mut console, mut vm, _log) = console();

    console.set_columns(250);
    vm.write_all(&frame(250)).unwrap();
    console.expect(&["Enter Boot"], SECOND).unwrap();

    let screen = console.screen();
    let first = screen.lines().next().unwrap_or_default();
    assert!(
        first.starts_with("BOOT ENVIRONMENTS") && first.ends_with("end"),
        "{first}"
    );
    assert_eq!(first.chars().count(), 250);
    assert_eq!(screen.lines().nth(24), Some("Enter Boot"));
}

#[test]
fn what_is_sent_reaches_the_virtual_machine() {
    let (mut console, mut vm, _log) = console();

    console.send("\x1b").unwrap();
    console.sendline("stty -echo").unwrap();
    let mut received = [0_u8; 12];
    vm.read_exact(&mut received).unwrap();

    assert_eq!(&received, b"\x1bstty -echo\n");
}
