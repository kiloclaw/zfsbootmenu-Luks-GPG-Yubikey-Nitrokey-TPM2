//! Tests of the console: prompts and passphrases on a terminal whose `stty`
//! is faked.
//!
//! They are integration tests because the fakes of `zbm-testkit` implement
//! the traits of this crate as other crates see it.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    reason = "integration tests set up real files and fail loudly when they cannot"
)]

use std::fs::File;
use std::time::Duration;

use zbm_platform::console::{Console, NoConsole, Size, Terminal, Timed, parse_size};
use zbm_platform::{Input, Output, Status};
use zbm_testkit::{FakeRunner, TempDir};

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

struct Setup {
    _dir: TempDir,
    runner: FakeRunner,
    input: File,
    output: File,
    output_path: std::path::PathBuf,
}

fn setup(input: &[u8], tty: bool) -> Setup {
    let dir = TempDir::new("zbm-console");
    let input_path = dir.write("input", input);
    let output_path = dir.write("output", "");
    let runner = FakeRunner::new();
    if tty {
        runner.respond("stty -g", 0, "500:5:bf:8a3b\n");
        runner.respond("stty size", 0, "25 80\n");
    } else {
        runner.respond_program("stty", 1, "");
    }
    Setup {
        input: File::open(input_path).unwrap(),
        output: std::fs::OpenOptions::new()
            .write(true)
            .open(&output_path)
            .unwrap(),
        output_path,
        runner,
        _dir: dir,
    }
}

#[test]
fn keys_switch_the_terminal_and_restore_it() {
    let setup = setup(b"\x1bx", true);
    let terminal = Terminal::new(&setup.runner, setup.input, setup.output);
    assert_eq!(
        terminal.read_key(Some(Duration::from_secs(1))).unwrap(),
        Some(0x1b)
    );
    assert_eq!(terminal.read_key(None).unwrap(), Some(b'x'));
    // The end of the input ends the wait.
    assert_eq!(
        terminal.read_key(Some(Duration::from_secs(1))).unwrap(),
        None
    );
    assert_eq!(
        setup.runner.lines()[..3],
        ["stty -g", "stty -icanon -echo", "stty 500:5:bf:8a3b"]
    );
    assert!(
        setup
            .runner
            .calls()
            .iter()
            .all(|call| *call.get_stdin() == Input::Inherit)
    );
}

#[test]
fn secrets_are_read_without_echo() {
    let setup = setup(b"hunter2\nnext line\npartial", true);
    let terminal = Terminal::new(&setup.runner, setup.input, setup.output);
    let secret = terminal.read_secret().unwrap().unwrap();
    assert_eq!(secret.expose(), b"hunter2");
    assert_eq!(
        setup.runner.lines(),
        ["stty -g", "stty -echo", "stty 500:5:bf:8a3b"]
    );
    // The rest of the input stays for the next reader.
    assert_eq!(terminal.read_line().unwrap().as_deref(), Some("next line"));
    assert_eq!(terminal.read_line().unwrap(), None);
}

#[test]
fn timed_reads_wait_for_a_line() {
    let setup = setup(b"pw\npartial", true);
    let terminal = Terminal::new(&setup.runner, setup.input, setup.output);
    terminal.set_echo(false);
    let second = Duration::from_secs(1);
    match terminal.read_secret_within(second).unwrap() {
        Timed::Read(secret) => assert_eq!(secret.expose(), b"pw"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(terminal.read_secret_within(second).unwrap(), Timed::Ended);
    terminal.set_echo(true);
    assert_eq!(setup.runner.lines(), ["stty -echo", "stty echo"]);

    // A pipe that stays open without data times out.
    let mut child = std::process::Command::new("sleep")
        .arg("5")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pipe = File::from(std::os::fd::OwnedFd::from(child.stdout.take().unwrap()));
    let output = null_output();
    let terminal = Terminal::new(&setup.runner, pipe, output);
    let short = Duration::from_millis(50);
    assert_eq!(terminal.read_secret_within(short).unwrap(), Timed::TimedOut);
    child.kill().unwrap();
    child.wait().unwrap();
}

fn null_output() -> File {
    std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .unwrap()
}

#[test]
fn without_a_terminal_input_is_read_as_it_comes() {
    let setup = setup(b"pw\n", false);
    let terminal = Terminal::new(&setup.runner, setup.input, setup.output);
    assert_eq!(terminal.read_secret().unwrap().unwrap().expose(), b"pw");
    // Only the attempt to save the settings ran.
    assert_eq!(setup.runner.lines(), ["stty -g"]);
    assert!(terminal.size().is_err());
}

#[test]
fn writes_and_sizes() {
    let setup = setup(b"", true);
    let terminal = Terminal::new(&setup.runner, setup.input, setup.output);
    terminal.write(b"Booting ").unwrap();
    terminal.write(b"tank/ROOT/void\n").unwrap();
    assert_eq!(
        std::fs::read(&setup.output_path).unwrap(),
        b"Booting tank/ROOT/void\n"
    );
    assert_eq!(
        terminal.size().unwrap(),
        Size {
            rows: 25,
            columns: 80
        }
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
    let runner = FakeRunner::new();
    runner.on(|_| Some(Ok(Output::new(Status::exited(1), ""))));
    assert!(Terminal::standard(&runner).is_ok());
}
