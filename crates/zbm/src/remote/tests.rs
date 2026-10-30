#![allow(
    clippy::disallowed_methods,
    reason = "the session's socket is a real one, in a directory of the test's own, \
              served on its own thread"
)]

use std::io::Read as _;
use std::os::fd::AsFd as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use pretty_assertions::assert_eq;
use zbm_app::remote::{TAKEN_OVER, ToMenu, ToSession};
use zbm_app::session::{SessionSocket, ask_remote};
use zbm_config::args::ZbmArgs;
use zbm_platform::event::spawn_scoped_traced;
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use super::*;

const WAIT: Option<Duration> = Some(Duration::from_secs(10));

/// The configuration of a machine with the default boot environment
/// `tank/ROOT/void`.
fn conf() -> Conf {
    let (options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(""));
    Conf::parse(&options.conf("/zfsbootmenu"))
}

/// A machine whose start-up is over, with no pools.
fn started() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.file("/dev/zfs", "");
    let state = State::new(&fake.files, "/zfsbootmenu");
    state.create().unwrap();
    state.mark_initialized().unwrap();
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    fake
}

/// A path for the session's socket, in a directory of its own.
fn socket_path(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("zbm-remote-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    directory.join("session.sock")
}

/// A terminal of pipes, its ends: what the session reads and writes, and
/// what the test writes and reads.
struct Pipes {
    input: io::PipeReader,
    output: io::PipeWriter,
    screen: io::PipeReader,
    keys: io::PipeWriter,
}

fn pipes() -> Pipes {
    let (input, keys) = io::pipe().unwrap();
    let (screen, output) = io::pipe().unwrap();
    Pipes {
        input,
        output,
        screen,
        keys,
    }
}

/// A menu that asks the session at `path` for one, and attaches with
/// `terminal`.
fn attached(path: &Path, terminal: &Pipes) -> Connection {
    let session = ask_remote(path).unwrap();
    ToSession::Attach
        .send(&session, &[terminal.input.as_fd(), terminal.output.as_fd()])
        .unwrap();
    assert_eq!(
        ToMenu::receive(&session, WAIT).unwrap(),
        Some(ToMenu::Attached)
    );
    session
}

#[test]
fn the_session_runs_the_menu_program_for_a_remote_menu_on_its_terminal() {
    let fake = started();
    let conf = conf();
    let path = socket_path("served");
    let socket = SessionSocket::bind(&path).unwrap();
    let token = CancelToken::new().unwrap();
    let remotes = Remotes {
        system: fake.system(),
        conf: &conf,
        handover: socket.handover(),
        until: &token,
        runner: Some(&fake.runner),
    };
    let remote = |peer: Connection| remotes.serve(&peer);
    let state = State::new(&fake.files, "/zfsbootmenu");

    let (first, told, second) = std::thread::scope(|scope| {
        let server = spawn_scoped_traced(scope, || socket.serve_with(&token, Some(&remote)));
        let terminal = pipes();
        let menu = attached(&path, &terminal);
        ToSession::Read.send(&menu, &[]).unwrap();
        let first = ToMenu::receive(&menu, WAIT).unwrap();
        assert!(state.is_active());

        // Another menu takes it over: the first is told.
        let other = pipes();
        let taker = attached(&path, &other);
        let told = ToMenu::receive(&menu, WAIT).unwrap();
        ToSession::Read.send(&taker, &[]).unwrap();
        let second = ToMenu::receive(&taker, WAIT).unwrap();
        ToSession::Detach.send(&taker, &[]).unwrap();
        // Served, the menu program let go of the lock.
        while socket.handover().is_asked() || state.is_active() {
            std::thread::sleep(Duration::from_millis(1));
        }
        token.cancel();
        server.join().unwrap();
        drop((terminal.output, other.output));
        (first, told, second)
    });

    assert!(
        matches!(first, Some(ToMenu::Snapshot(Some(_)))),
        "{first:?}"
    );
    assert_eq!(told, Some(ToMenu::Refused(TAKEN_OVER.to_owned())));
    assert!(
        matches!(second, Some(ToMenu::Snapshot(Some(_)))),
        "{second:?}"
    );
    assert!(!state.is_active());
}

#[test]
fn a_remote_menu_is_told_when_the_menu_program_ends_before_its_menu() {
    let fake = FakeSystem::new();
    fake.files.file("/dev/zfs", "");
    let conf = conf();
    let path = socket_path("ended");
    let socket = SessionSocket::bind(&path).unwrap();
    let token = CancelToken::new().unwrap();
    // Its programs on the menu's terminal, as in the image: none runs.
    let remotes = Remotes {
        system: fake.system(),
        conf: &conf,
        handover: socket.handover(),
        until: &token,
        runner: None,
    };
    let remote = |peer: Connection| remotes.serve(&peer);

    let (told, mut screen) = std::thread::scope(|scope| {
        let server = spawn_scoped_traced(scope, || socket.serve_with(&token, Some(&remote)));
        let terminal = pipes();
        let menu = attached(&path, &terminal);
        // Start-up is not over: the menu program asks on the terminal,
        // and the user stops it.
        std::io::Write::write_all(&mut &terminal.keys, b"\x1b").unwrap();
        let told = ToMenu::receive(&menu, WAIT).unwrap();
        // A menu that sends something else first is not served.
        let other = ask_remote(&path).unwrap();
        ToSession::Read.send(&other, &[]).unwrap();
        assert!(matches!(
            ToMenu::receive(&other, WAIT).unwrap(),
            Some(ToMenu::Refused(_))
        ));
        token.cancel();
        server.join().unwrap();
        drop(terminal.output);
        (told, terminal.screen)
    });

    assert_eq!(told, Some(ToMenu::Refused(NOT_SERVED.to_owned())));
    let mut text = String::new();
    screen.read_to_string(&mut text).unwrap();
    assert!(text.contains("ZFSBootMenu must be initialized"), "{text}");
}

#[test]
fn the_menu_over_ssh_is_served_until_the_session_s_own_menu_takes_it_back() {
    use zbm_tui::menu::machine::{Machine as _, RemoteMachine};
    use zbm_tui::menu::world::{Event, Outcome};

    let fake = started();
    let conf = conf();
    let path = socket_path("client");
    let socket = SessionSocket::bind(&path).unwrap();
    let token = CancelToken::new().unwrap();
    let remotes = Remotes {
        system: fake.system(),
        conf: &conf,
        handover: socket.handover(),
        until: &token,
        runner: Some(&fake.runner),
    };
    let remote = |peer: Connection| remotes.serve(&peer);
    let state = State::new(&fake.files, "/zfsbootmenu");
    let never = || false;

    let (performed, left) = std::thread::scope(|scope| {
        let server = spawn_scoped_traced(scope, || socket.serve_with(&token, Some(&remote)));
        let terminal = pipes();
        let session = ask_remote(&path).unwrap();
        let fds = (terminal.input.as_fd(), terminal.output.as_fd());
        let mut machine = RemoteMachine::attach(session, fds, &never).unwrap();
        assert!(matches!(machine.tick().as_slice(), [Event::Changed(_)]));
        assert!(state.is_active());

        machine.apply(Action::ResetBootTries("tank/ROOT/void".parse().unwrap()));
        let mut performed = Vec::new();
        while machine.is_busy() {
            std::thread::sleep(Duration::from_millis(1));
            performed.extend(machine.poll());
        }

        // The console takes the menu back.
        assert!(socket.handover().has_yielded(Duration::from_secs(10)));
        assert!(!state.is_active());
        let mut left = Vec::new();
        while left.is_empty() {
            std::thread::sleep(Duration::from_millis(1));
            left = machine.poll();
        }
        drop(machine);
        token.cancel();
        server.join().unwrap();
        (performed, left)
    });

    assert!(
        performed.iter().any(|event| matches!(
            event,
            Event::Finished(Outcome::Done(_) | Outcome::Failed { .. })
        )),
        "{performed:?}"
    );
    assert_eq!(left, [Event::Leaving(String::new())]);
}
