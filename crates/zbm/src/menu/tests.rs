use zbm_config::args::ZbmArgs;
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// The configuration of a machine with the default boot environment
/// `tank/ROOT/void`.
fn conf() -> Conf {
    let (options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(""));
    Conf::parse(&options.conf("/zfsbootmenu")).unwrap()
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
    assert_eq!(menu(fake.system(), &conf(), ended), Ok(0));
    assert_eq!(
        menu(fake.system(), &conf(), no_terminal),
        Err("not a terminal".to_owned())
    );
    assert!(!is_taken_over());

    let broken = started(1);
    let error = menu(broken.system(), &conf(), ended).unwrap_err();
    assert!(!error.is_empty());
}
