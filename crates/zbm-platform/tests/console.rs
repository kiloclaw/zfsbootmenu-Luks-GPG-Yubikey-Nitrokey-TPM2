//! Tests of the console: prompts and passphrases on a pseudo-terminal, whose
//! settings the tests read back.
//!
//! They are integration tests because the fakes of `zbm-testkit` implement
//! the traits of this crate as other crates see it.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    reason = "integration tests set up real files and fail loudly when they cannot"
)]

use std::fs::File;
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt as _;
use std::time::Duration;

use rustix::termios::{self, LocalModes};
use zbm_platform::console::{Console, NoConsole, Screen, Size, Terminal, Timed, parse_size};
use zbm_testkit::TempDir;

#[test]
fn no_console_reads_nothing_and_shows_nothing() {
    let console = NoConsole;

    assert!(console.write(b"x").is_ok());
    assert!(console.size().is_err());
    assert_eq!(console.read_key(None).unwrap(), None);
    assert!(console.read_secret().unwrap().is_none());
    assert_eq!(console.read_line().unwrap(), None);
    console.set_echo(false);
    assert!(matches!(
        console.read_secret_within(Duration::from_secs(1)).unwrap(),
        Timed::Ended
    ));
}

/// A pseudo-terminal: its controlling side, where the test types, and the
/// terminal the console reads.
fn pty() -> (File, File) {
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    let flags = OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC;
    let master = openpt(flags).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let terminal = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(std::ffi::OsStr::from_bytes(name.as_bytes()))
        .unwrap();
    (File::from(master), terminal)
}

/// The local modes of `terminal`.
fn modes(terminal: &File) -> LocalModes {
    termios::tcgetattr(terminal).unwrap().local_modes
}

/// A file with `input`, read as a console that is no terminal.
fn file_input(dir: &TempDir, input: &[u8]) -> File {
    File::open(dir.write("input", input)).unwrap()
}

fn null_output() -> File {
    std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .unwrap()
}

#[test]
fn keys_switch_the_terminal_and_restore_it() {
    let (mut master, input) = pty();
    let watched = input.try_clone().unwrap();
    let before = modes(&watched);
    assert!(before.contains(LocalModes::ICANON | LocalModes::ECHO));
    let terminal = Terminal::new(input, null_output());

    // Without Enter: the terminal is not canonical while the key is read.
    master.write_all(b"\x1bx").unwrap();
    assert_eq!(
        terminal.read_key(Some(Duration::from_secs(5))).unwrap(),
        Some(0x1b)
    );
    assert_eq!(terminal.read_key(None).unwrap(), Some(b'x'));
    assert_eq!(modes(&watched), before);
    // Nothing typed: the wait ends with the time.
    let short = Duration::from_millis(20);
    assert_eq!(terminal.read_key(Some(short)).unwrap(), None);
    assert_eq!(modes(&watched), before);
}

#[test]
fn secrets_are_read_without_echo_and_echo_follows() {
    let (mut master, input) = pty();
    let watched = input.try_clone().unwrap();
    let terminal = Terminal::new(input, null_output());

    master.write_all(b"hunter2\nnext line\n").unwrap();
    let secret = terminal.read_secret().unwrap().unwrap();
    assert_eq!(secret.expose(), b"hunter2");
    assert!(modes(&watched).contains(LocalModes::ECHO));
    // The rest of the input stays for the next reader.
    assert_eq!(terminal.read_line().unwrap().as_deref(), Some("next line"));

    terminal.set_echo(false);
    assert!(!modes(&watched).contains(LocalModes::ECHO));
    master.write_all(b"pw\n").unwrap();
    match terminal.read_secret_within(Duration::from_secs(5)).unwrap() {
        Timed::Read(secret) => assert_eq!(secret.expose(), b"pw"),
        other => panic!("unexpected {other:?}"),
    }
    terminal.set_echo(true);
    assert!(modes(&watched).contains(LocalModes::ECHO));
}

#[test]
fn the_size_is_the_terminal_s_and_a_serial_one_is_told_its_own() {
    let (master, input) = pty();
    let size = |rows, columns| Size { rows, columns };
    let mut winsize = termios::tcgetwinsize(&master).unwrap();
    winsize.ws_row = 25;
    winsize.ws_col = 80;
    termios::tcsetwinsize(&master, winsize).unwrap();
    let terminal = Terminal::new(input, null_output());

    assert_eq!(terminal.size().unwrap(), size(25, 80));
    terminal.resize(size(40, 132)).unwrap();
    assert_eq!(terminal.size().unwrap(), size(40, 132));
    let seen = termios::tcgetwinsize(&master).unwrap();
    assert_eq!((seen.ws_row, seen.ws_col), (40, 132));
}

#[test]
fn the_size_of_a_terminal_device_is_read_by_its_path() {
    use zbm_platform::files::{Files as _, RealFiles};
    let (master, terminal) = pty();
    let mut winsize = termios::tcgetwinsize(&master).unwrap();
    winsize.ws_row = 67;
    winsize.ws_col = 240;
    termios::tcsetwinsize(&master, winsize).unwrap();
    let name = rustix::pty::ptsname(&master, Vec::new()).unwrap();
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(name.as_bytes()));

    assert_eq!(
        RealFiles.terminal_size(path).unwrap(),
        Size {
            rows: 67,
            columns: 240
        }
    );
    drop(terminal);
    let dir = TempDir::new("zbm-console");
    assert!(RealFiles.terminal_size(&dir.write("file", "")).is_err());
    assert!(RealFiles.terminal_size(&dir.join("none")).is_err());
}

#[test]
fn timed_reads_time_out_on_a_pipe_without_data() {
    let mut child = std::process::Command::new("sleep")
        .arg("5")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pipe = File::from(std::os::fd::OwnedFd::from(child.stdout.take().unwrap()));
    let terminal = Terminal::new(pipe, null_output());
    let short = Duration::from_millis(50);
    assert_eq!(terminal.read_secret_within(short).unwrap(), Timed::TimedOut);
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn without_a_terminal_input_is_read_as_it_comes() {
    let dir = TempDir::new("zbm-console");
    let terminal = Terminal::new(file_input(&dir, b"pw\npartial"), null_output());
    terminal.set_echo(false);
    assert_eq!(terminal.read_secret().unwrap().unwrap().expose(), b"pw");
    assert_eq!(
        terminal.read_secret_within(Duration::from_secs(1)).unwrap(),
        Timed::Ended
    );
    assert!(terminal.size().is_err());
    assert!(
        terminal
            .resize(Size {
                rows: 25,
                columns: 80
            })
            .is_err()
    );
    assert_eq!(
        NoConsole
            .resize(Size {
                rows: 25,
                columns: 80
            })
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::Unsupported
    );
}

#[test]
fn writes_and_screen_changes_reach_the_output() {
    let dir = TempDir::new("zbm-console");
    let output_path = dir.write("output", "");
    let output = std::fs::OpenOptions::new()
        .write(true)
        .open(&output_path)
        .unwrap();
    let terminal = Terminal::new(file_input(&dir, b""), output);
    terminal.write(b"Booting ").unwrap();
    terminal.write(b"tank/ROOT/void\n").unwrap();
    terminal.screen(Screen::Clear);
    terminal.screen(Screen::HideCursor);
    terminal.screen(Screen::MoveTo { row: 0, column: 9 });
    terminal.screen(Screen::ShowCursor);
    assert_eq!(
        std::fs::read(&output_path).unwrap(),
        b"Booting tank/ROOT/void\n\x1b[H\x1b[2J\x1b[?25l\x1b[1;10H\x1b[?25h"
    );
}

#[test]
fn parses_stty_sizes() {
    assert_eq!(
        parse_size("50 132\n"),
        Some(Size {
            rows: 50,
            columns: 132
        })
    );
    assert_eq!(parse_size(""), None);
    assert_eq!(parse_size("50"), None);
    assert_eq!(parse_size("50 x"), None);
    assert_eq!(parse_size("1 2 3"), None);
}

#[test]
fn the_standard_terminal_can_be_opened() {
    assert!(Terminal::standard().is_ok());
}
