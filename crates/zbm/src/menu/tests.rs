use zbm_config::args::ZbmArgs;
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use super::*;

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
fn ended(session: Session<RealMachine<'_, '_>>) -> io::Result<String> {
    assert!(session.model().snapshot.environments.is_empty());
    Ok(String::new())
}

/// A menu whose terminal cannot be used.
fn no_terminal(_session: Session<RealMachine<'_, '_>>) -> io::Result<String> {
    Err(io::Error::other("not a terminal"))
}

#[test]
fn the_menu_runs_on_the_machine_and_says_why_it_could_not() {
    let fake = started(0);
    assert_eq!(menu(fake.system(), &conf(), ended, Countdown::Skip), Ok(0));
    assert_eq!(
        menu(fake.system(), &conf(), no_terminal, Countdown::Run),
        Err("not a terminal".to_owned())
    );
    assert!(!is_taken_over());

    let broken = started(1);
    let error = menu(broken.system(), &conf(), ended, Countdown::Skip).unwrap_err();
    assert!(!error.is_empty());
}

/// A menu that checks that it counts down, then ends.
fn counting(session: Session<RealMachine<'_, '_>>) -> io::Result<String> {
    let snapshot = &session.model().snapshot;
    assert_eq!(snapshot.bootfs, Some("tank/ROOT/void".parse().unwrap()));
    assert_eq!(snapshot.timeout, zbm_tui::menu::world::Timeout::Seconds(10));
    Ok(String::new())
}

/// A menu that checks that it waits for the user, then ends.
fn waiting(session: Session<RealMachine<'_, '_>>) -> io::Result<String> {
    assert_eq!(
        session.model().snapshot.timeout,
        zbm_tui::menu::world::Timeout::Wait
    );
    Ok(String::new())
}

#[test]
fn the_menu_counts_down_only_at_the_end_of_start_up_with_a_chosen_environment() {
    let fake = started(0);
    assert_eq!(menu(fake.system(), &conf(), waiting, Countdown::Run), Ok(0));
    State::new(&fake.files, "/zfsbootmenu")
        .set_bootfs("tank/ROOT/void")
        .unwrap();
    assert_eq!(
        menu(fake.system(), &conf(), counting, Countdown::Run),
        Ok(0)
    );
    assert_eq!(
        menu(fake.system(), &conf(), waiting, Countdown::Skip),
        Ok(0)
    );
}

#[test]
fn the_menu_takes_only_the_countdown_option() {
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
