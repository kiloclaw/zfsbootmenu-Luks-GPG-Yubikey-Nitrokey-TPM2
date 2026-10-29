#![allow(
    clippy::needless_pass_by_value,
    clippy::unnecessary_wraps,
    reason = "the stand-in menus have the signature of the menu's `Show`"
)]

use zbm_config::args::ZbmArgs;
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use zbm_tui::menu::machine::Machine as _;

/// The configuration of a machine with the default boot environment
/// `tank/ROOT/void`.
fn conf() -> Conf {
    let (options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(""));
    Conf::parse(&options.conf("/zfsbootmenu"))
}

/// A machine whose start-up is over, with no pools; `zfs` and `zpool` exit
/// with `status`.
fn started(status: u8) -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.file("/dev/zfs", "");
    let state = State::new(&fake.files, "/zfsbootmenu");
    state.create().unwrap();
    state.mark_initialized().unwrap();
    fake.runner.respond_program("zpool", status, "");
    fake.runner.respond_program("zfs", status, "");
    fake
}

/// A menu that ends at once, having seen the machine.
fn ended(machine: RealMachine<'_, '_>) -> io::Result<String> {
    assert!(machine.snapshot().environments.is_empty());
    Ok(String::new())
}

/// A menu whose terminal cannot be used.
fn no_terminal(_machine: RealMachine<'_, '_>) -> io::Result<String> {
    Err(io::Error::other("not a terminal"))
}

#[test]
fn the_menu_runs_on_the_machine_and_says_why_it_could_not() {
    let fake = started(0);
    assert_eq!(
        menu(fake.system(), &conf(), (&mut ended, Countdown::Skip), None),
        Ok(0)
    );
    assert_eq!(
        menu(
            fake.system(),
            &conf(),
            (&mut no_terminal, Countdown::Run),
            None
        ),
        Err("not a terminal".to_owned())
    );
    assert!(!is_taken_over(SignalSet::EMPTY));
    assert!(!is_taken_over(SignalSet::of(&[Signal::Interrupt])));
    for signal in [Signal::User1, Signal::Terminate, Signal::Hangup] {
        assert!(is_taken_over(SignalSet::of(&[signal])), "{signal:?}");
    }

    let broken = started(1);
    let error = menu(
        broken.system(),
        &conf(),
        (&mut ended, Countdown::Skip),
        None,
    )
    .unwrap_err();
    assert!(!error.is_empty());
}

/// A menu that checks that it counts down, then ends.
fn counting(machine: RealMachine<'_, '_>) -> io::Result<String> {
    let snapshot = machine.snapshot();
    assert_eq!(snapshot.bootfs, Some("tank/ROOT/void".parse().unwrap()));
    assert_eq!(snapshot.timeout, zbm_tui::menu::world::Timeout::Seconds(10));
    Ok(String::new())
}

/// A menu that checks that it waits for the user, then ends.
fn waiting(machine: RealMachine<'_, '_>) -> io::Result<String> {
    assert_eq!(
        machine.snapshot().timeout,
        zbm_tui::menu::world::Timeout::Wait
    );
    Ok(String::new())
}

#[test]
fn the_menu_counts_down_only_at_the_end_of_start_up_with_a_chosen_environment() {
    let fake = started(0);
    assert_eq!(
        menu(fake.system(), &conf(), (&mut waiting, Countdown::Run), None),
        Ok(0)
    );
    State::new(&fake.files, "/zfsbootmenu")
        .set_bootfs("tank/ROOT/void")
        .unwrap();
    assert_eq!(
        menu(
            fake.system(),
            &conf(),
            (&mut counting, Countdown::Run),
            None
        ),
        Ok(0)
    );
    assert_eq!(
        menu(
            fake.system(),
            &conf(),
            (&mut waiting, Countdown::Skip),
            None
        ),
        Ok(0)
    );
}

#[test]
fn the_menu_takes_no_option() {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut io = Io {
        stdout: &mut out,
        stderr: &mut err,
    };
    assert_eq!(run(&["--bogus".to_owned()], &mut io), crate::USAGE);
    assert_eq!(
        String::from_utf8_lossy(&err),
        "zfsbootmenu: unknown options --bogus\n"
    );
}

/// The session's menu: it reads the machine until it is told to leave.
struct UntilLeaving(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl MenuScreen for UntilLeaving {
    #[allow(
        clippy::disallowed_methods,
        reason = "the menu waits in real time for the socket's thread"
    )]
    fn run(&mut self, mut machine: RealMachine<'_, '_>) -> io::Result<String> {
        use std::sync::atomic::Ordering;
        self.0.store(true, Ordering::Release);
        loop {
            let events = machine.tick();
            if events
                .iter()
                .any(|event| matches!(event, zbm_tui::menu::world::Event::Leaving(_)))
            {
                return Ok(String::new());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "the session's socket is a real one, in a directory of the test's own"
)]
fn the_session_s_menu_ends_when_another_instance_asks_by_the_socket() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use zbm_app::session::{Answer, SessionSocket, ask_takeover};
    use zbm_platform::event::{CancelToken, spawn_scoped_traced};

    let fake = started(0);
    let directory = std::env::temp_dir().join(format!("zbm-menu-session-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let path = directory.join("session.sock");
    let socket = SessionSocket::bind(&path).unwrap();
    let running = Arc::new(AtomicBool::new(false));
    let conf = conf();
    let mut session = InSession {
        screen: Box::new(UntilLeaving(running.clone())),
        system: fake.system(),
        conf: &conf,
        signals: None,
        handover: Some(socket.handover()),
    };
    let startup = Startup::new(fake.system(), &conf);
    let token = CancelToken::new().unwrap();

    let answer = std::thread::scope(|scope| {
        spawn_scoped_traced(scope, || socket.serve(&token));
        let asked = spawn_scoped_traced(scope, || {
            while !running.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            ask_takeover(&path)
        });
        session.run(&startup, false);
        let answer = asked.join().unwrap();
        token.cancel();
        answer
    });

    assert_eq!(answer, Some(Answer::Yielded));
    assert!(!State::new(&fake.files, "/zfsbootmenu").is_active());
    assert!(!socket.handover().is_asked());
}
