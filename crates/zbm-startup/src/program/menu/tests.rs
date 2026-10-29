use zbm_boot::error::BootError;
use zbm_config::args::ZbmArgs;
use zbm_config::conf::Conf;
use zbm_config::options::ZbmOptions;
use zbm_platform::exec::{Output, Status};
use zbm_platform::files::Files;
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::program::Startup;

/// A menu that records the settings it was shown with, and opens one
/// recovery shell.
#[derive(Default)]
struct Recorded {
    shown: Vec<Vec<(String, String)>>,
}

impl Menu for Recorded {
    fn show(&mut self, settings: &BootSettings, recovery: &dyn Recovery) {
        self.shown.push(settings.env.clone());
        recovery.emergency_shell("");
    }
}

/// A menu that checks the lock while it runs.
struct Locked<'s, 'a>(&'s State<'a>, Vec<bool>);

impl Menu for Locked<'_, '_> {
    fn show(&mut self, _settings: &BootSettings, recovery: &dyn Recovery) {
        self.1.push(self.0.active_pid() == Some(4242));
        assert!(matches!(
            recovery.unlock_late(),
            Err(BootError::NoCryptsetup)
        ));
    }
}

fn conf() -> Conf {
    let (options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(""));
    Conf::parse(&options.conf("/zfsbootmenu"))
}

/// A machine whose start-up is over, with `/dev/zfs` and the default boot
/// environment `tank/ROOT/void`.
fn started() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.runner.respond("tput lines", 0, "25\n");
    fake.runner.respond("tput cols", 0, "80\n");
    fake.files.file("/dev/zfs", "");
    let state = State::new(&fake.files, "/zfsbootmenu");
    state.create().unwrap();
    state.mark_initialized().unwrap();
    state.set_bootfs("tank/ROOT/void").unwrap();
    fake
}

fn shells(fake: &FakeSystem) -> usize {
    fake.runner
        .calls()
        .iter()
        .filter(|call| call.program() == "/bin/bash")
        .count()
}

#[test]
fn the_menu_runs_after_the_setup_hooks_with_the_lock_held() {
    let fake = started();
    fake.files
        .file_with_mode("/libexec/hooks/setup.d/10-setup", "", 0o755);
    let startup = Startup::new(fake.system(), &conf());
    let mut menu = Recorded::default();
    assert_eq!(startup.menu(&mut menu), 0);

    // The hooks get the configuration's variables, then BOOTFS.
    let [env] = menu.shown.as_slice() else {
        panic!("{:?}", menu.shown);
    };
    let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
    assert_eq!(env.last(), Some(&pair("BOOTFS", "tank/ROOT/void")));
    assert!(env.contains(&pair("BASE", "/zfsbootmenu")), "{env:?}");
    assert!(env.contains(&pair("loglevel", "4")), "{env:?}");
    let hook = fake
        .runner
        .calls()
        .into_iter()
        .find(|call| call.program() == "/libexec/hooks/setup.d/10-setup")
        .expect("the setup hook ran");
    assert!(hook.get_env().iter().any(
        |(name, value)| name == "BOOTFS" && value.as_deref() == Some("tank/ROOT/void".as_ref())
    ));
    assert_eq!(shells(&fake), 1);
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert!(!state.is_active());

    let mut locked = Locked(&state, Vec::new());
    startup.menu(&mut locked);
    assert_eq!(locked.1, [true]);
    assert!(!state.is_active());
}

#[test]
fn without_zfs_the_shell_opens_first() {
    let fake = started();
    fake.files.remove_file(Path::new("/dev/zfs")).unwrap();
    let startup = Startup::new(fake.system(), &conf());
    startup.menu(&mut Recorded::default());
    assert!(
        fake.console
            .output_text()
            .contains("/dev/zfs missing, check that kernel modules are loaded")
    );
    assert_eq!(shells(&fake), 2);
}

#[test]
fn the_menu_waits_for_start_up_and_runs_it_when_asked() {
    let fake = started();
    let state = State::new(&fake.files, "/zfsbootmenu");
    fake.files
        .remove_file(&state.base().join("initialized"))
        .unwrap();
    fake.console.keys(&[0x1b]);
    let startup = Startup::new(fake.system(), &conf());
    let mut menu = Recorded::default();
    assert_eq!(startup.menu(&mut menu), 0);
    assert!(menu.shown.is_empty());
    assert!(
        fake.console
            .output_text()
            .contains("ZFSBootMenu must be initialized to continue")
    );

    // Start-up imports the pool tank.
    fake.runner.respond("zpool list -H -o name", 0, "tank\n");
    fake.runner
        .respond("zpool list -H -o bootfs --", 0, "tank/ROOT/void\n");
    fake.console.keys(b"\n");
    startup.menu(&mut menu);
    assert!(state.is_initialized());
    assert_eq!(menu.shown.len(), 1);
}

#[test]
fn another_menu_is_asked_to_end_and_waited_for() {
    let fake = started();
    let state = State::new(&fake.files, "/zfsbootmenu");
    // The other menu, 77, holds the lock.
    let _other = state.take_active(77).unwrap().unwrap();
    fake.files.file("/proc/77/task/77/children", "78\n");
    fake.console.keys(&[0x1b]);
    let startup = Startup::new(fake.system(), &conf());
    let mut menu = Recorded::default();
    assert_eq!(startup.menu(&mut menu), 0);
    assert!(menu.shown.is_empty());
    assert_eq!(
        fake.processes.signals(),
        [(77, Signal::User1), (78, Signal::Terminate)]
    );
    assert!(
        fake.console
            .output_text()
            .contains("Waiting for other ZFSBootMenu instance to terminate")
    );
    assert_eq!(state.active_pid(), Some(77));
}

#[test]
fn the_pid_a_killed_menu_left_is_not_trusted() {
    let fake = started();
    let state = State::new(&fake.files, "/zfsbootmenu");
    // The menu was killed (SIGKILL): the kernel let go of its lock, and its
    // PID stayed in `active`.
    fake.files.file("/zfsbootmenu/active", "77\n");
    fake.processes.end(77);
    let startup = Startup::new(fake.system(), &conf());
    let mut menu = Recorded::default();
    assert_eq!(startup.menu(&mut menu), 0);
    assert_eq!(menu.shown.len(), 1);
    assert!(fake.processes.signals().is_empty());
    assert!(!state.is_active());
}

#[test]
fn the_menu_program_is_found_when_executable() {
    let fake = FakeSystem::new();
    assert!(!has_menu(&fake.system()));
    fake.files.file(MENU, "");
    assert!(!has_menu(&fake.system()));
    fake.files.file_with_mode(MENU, "", 0o755);
    assert!(has_menu(&fake.system()));
}

#[test]
fn the_loop_runs_the_menu_program_then_the_shell() {
    let fake = started();
    let state = State::new(&fake.files, "/zfsbootmenu");
    for stage in zbm_types::Stage::ALL {
        state.finish_stage(stage.as_str()).unwrap();
    }
    fake.files.file_with_mode(MENU, "", 0o755);
    let resume = Modes {
        console: true,
        resume: true,
        ..Modes::default()
    };
    Startup::new(fake.system(), &conf()).run(resume, Some(1));
    let programs: Vec<String> = fake
        .runner
        .calls()
        .iter()
        .map(|call| call.program().to_string_lossy().into_owned())
        .filter(|program| program == MENU || program == "/bin/bash")
        .collect();
    assert_eq!(programs, [MENU, "/bin/bash"]);
    assert!(
        !fake
            .console
            .output_text()
            .contains(zbm_boot::support::MESSAGE)
    );
}

/// A session's menu that records whether each round counted down.
#[derive(Default)]
struct Rounds(Vec<bool>);

impl SessionMenu for Rounds {
    fn run(&mut self, _: &Startup<'_>, countdown: bool) {
        self.0.push(countdown);
    }
}

/// A machine whose start-up is over, with the menu program.
fn finished() -> FakeSystem {
    let fake = started();
    let state = State::new(&fake.files, "/zfsbootmenu");
    for stage in zbm_types::Stage::ALL {
        state.finish_stage(stage.as_str()).unwrap();
    }
    fake.files.file_with_mode(MENU, "", 0o755);
    fake
}

const CONSOLE: Modes = Modes {
    console: true,
    rerun: crate::step::Rerun::Pending,
    resume: false,
};

#[test]
fn the_session_s_menu_counts_down_the_first_time_and_the_program_does_not() {
    let fake = finished();
    let startup = Startup::new(fake.system(), &conf());
    let mut rounds = Rounds::default();

    let bootfs = startup.start(CONSOLE, None);
    startup.after_stages(CONSOLE, Some(2), bootfs, Some(&mut rounds));

    assert_eq!(rounds.0, [true, false]);
    assert!(menu_calls(&fake).is_empty(), "{:?}", menu_calls(&fake));
    assert!(!fake.console.output_text().contains("Booting"));

    // Resumed, the menu waits for the user.
    let mut rounds = Rounds::default();
    let resume = Modes {
        resume: true,
        ..CONSOLE
    };
    startup.after_stages(resume, Some(1), None, Some(&mut rounds));
    assert_eq!(rounds.0, [false]);
}

/// The calls of the menu program, as command lines.
fn menu_calls(fake: &FakeSystem) -> Vec<String> {
    fake.runner
        .calls()
        .iter()
        .filter(|call| call.program() == MENU)
        .map(zbm_platform::Command::display)
        .collect()
}

#[test]
fn without_the_session_start_up_counts_down_and_runs_the_menu_program() {
    // A menu that cannot start, crashes or ends: the shell follows it.
    for status in [Status::exited(1), Status::exited(101), Status::from_raw(11)] {
        let fake = finished();
        fake.runner
            .on(move |command| (command.program() == MENU).then(|| Ok(Output::new(status, ""))));

        Startup::new(fake.system(), &conf()).run(CONSOLE, Some(2));

        assert_eq!(menu_calls(&fake), [MENU.to_owned(), MENU.to_owned()]);
        let shown = fake.console.output_text();
        // One countdown, from ten: start-up's, before the first round.
        assert_eq!(
            shown.matches("in \x1b[1;33m10").count(),
            1,
            "{status:?}: {shown}"
        );
        assert_eq!(shown.matches("emergency shell").count(), 2, "{status:?}");
    }
}

#[test]
fn after_another_instance_was_seen_nothing_boots_by_itself() {
    let fake = started();
    let state = State::new(&fake.files, "/zfsbootmenu");
    for stage in zbm_types::Stage::ALL {
        state.finish_stage(stage.as_str()).unwrap();
    }
    fake.files.file_with_mode(MENU, "", 0o755);
    let _other = state.take_active(77).unwrap().unwrap();
    let console = Modes {
        console: true,
        ..Modes::default()
    };
    Startup::new(fake.system(), &conf()).run(console, Some(1));
    assert_eq!(menu_calls(&fake), [MENU.to_owned()]);
}
