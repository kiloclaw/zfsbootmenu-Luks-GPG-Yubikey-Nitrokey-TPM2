use super::*;
use zbm_boot::error::BootError;
use zbm_platform::KernelLog;
use zbm_platform::exec::{Input, Signal};
use zbm_testkit::fakes::FakeSystem;
use zbm_types::Stage;

/// The configuration the start-up would write for `cmdline`.
fn conf(cmdline: &str) -> Conf {
    let (options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(cmdline));
    Conf::parse(&options.conf("/zfsbootmenu")).unwrap()
}

/// A machine with one pool and one boot environment with a kernel.
fn machine() -> FakeSystem {
    machine_with_health("tank\tONLINE\n")
}

/// The machine, with `zpool list -H -o name,health` giving `health`.
fn machine_with_health(health: &str) -> FakeSystem {
    let fake = FakeSystem::new();
    let runner = &fake.runner;
    runner.respond("tput lines", 0, "25\n");
    runner.respond("tput cols", 0, "80\n");
    runner.respond("zpool list -H -o name", 0, "tank\n");
    runner.respond("zpool list -H -o name,health", 0, health);
    runner.respond("zpool list -H -o bootfs --", 0, "tank/ROOT/void\n");
    runner.respond(
        "zfs get -H -o value org.zfsbootmenu:kernel tank/ROOT/void",
        0,
        "-\n",
    );
    runner.respond("zpool get -H -o value readonly tank", 0, "on\n");
    let mnt = "/zfsbootmenu/environments/tank/ROOT/void/mnt";
    fake.files
        .file(format!("{mnt}/boot/vmlinuz-6.1"), "")
        .file(format!("{mnt}/boot/initramfs-6.1.img"), "")
        .dir("/libexec/hooks/early-setup.d");
    fake
}

fn startup<'a>(fake: &'a FakeSystem, cmdline: &str) -> Startup<'a> {
    Startup::new(fake.system(), &conf(cmdline))
}

fn has(fake: &FakeSystem, line: &str) -> bool {
    fake.runner.lines().iter().any(|run| run == line)
}

fn shells(fake: &FakeSystem) -> Vec<String> {
    fake.runner
        .calls()
        .into_iter()
        .filter(|call| call.program() == "/bin/bash")
        .map(|call| {
            assert_eq!(call.get_default_signals(), [Signal::Interrupt]);
            call.display()
        })
        .collect()
}

#[test]
fn the_steps_are_the_stages_in_order() {
    let fake = machine();
    let program = startup(&fake, "loglevel=7");
    let stages: Vec<Stage> = program.steps().iter().map(|step| step.stage()).collect();
    assert_eq!(stages, Stage::ALL);
}

#[test]
fn values_that_turn_modes_on() {
    for value in ["yes", "Y", "TRUE", "t", "1"] {
        assert!(is_on(value), "{value}");
    }
    for value in ["", "no", "0", "on", "2"] {
        assert!(!is_on(value), "{value}");
    }
}

#[test]
fn stages_run_once_unless_forced() {
    let fake = machine();
    let program = startup(&fake, "loglevel=7");
    assert_eq!(program.run(Modes::default(), Some(0)), 0);
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert_eq!(
        state.finished_stages(),
        Stage::ALL
            .iter()
            .map(|stage| stage.as_str())
            .collect::<Vec<_>>()
    );
    assert!(state.is_initialized());
    assert_eq!(state.bootfs().as_deref(), Some("tank/ROOT/void"));
    for line in [
        "modprobe spl",
        "modprobe zfs",
        "udevadm settle",
        "tput clear",
        "zpool import -N -o readonly=on -a",
    ] {
        assert!(has(&fake, line), "{line}");
    }
    let kmsg = fake.kmsg();
    let order: Vec<usize> = Stage::ALL
        .iter()
        .map(|stage| kmsg.find(&format!("running init stage {stage}")).unwrap())
        .collect();
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(shells(&fake).is_empty());

    // A second run has nothing to do; a forced one runs every stage again.
    let calls = fake.runner.lines().len();
    program.run(Modes::default(), Some(0));
    assert!(!fake.runner.lines()[calls..].contains(&"modprobe zfs".to_owned()));
    let forced = Modes {
        rerun: Rerun::All,
        ..Modes::default()
    };
    program.run(forced, Some(0));
    assert!(fake.runner.lines()[calls..].contains(&"modprobe zfs".to_owned()));
    assert_eq!(state.finished_stages().len(), 12);
}

#[test]
fn the_console_counts_down_and_boots() {
    let fake = machine();
    let console = Modes {
        console: true,
        ..Modes::default()
    };
    startup(&fake, "zbm.timeout=0").run(console, Some(1));
    let lines = fake.runner.lines();
    let load = lines
        .iter()
        .position(|line| line.starts_with("kexec -a -l "))
        .expect("the kernel is loaded");
    assert!(lines[load].contains("/mnt/boot/vmlinuz-6.1 --initrd="));
    let execute = lines.iter().position(|line| line == "kexec -e -i").unwrap();
    assert!(load < execute);
    // The fake kexec returns, so the boot failed: the loop's shell follows.
    assert!(fake.kmsg().contains("<3>ZFSBootMenu: kexec -e -i failed!"));
    assert_eq!(shells(&fake), ["/bin/bash -i"]);
    let output = fake.console.output_text();
    assert!(output.contains("Booting \x1b[1;33m/boot/vmlinuz-6.1\x1b[0m"));
    assert!(output.contains(support::MESSAGE));
}

#[test]
fn the_logs_are_saved_before_the_kernel_starts_when_asked() {
    let fake = machine();
    fake.runner.respond(
        "lsblk -rnpo NAME,PARTTYPE",
        0,
        "/dev/vda1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n",
    );
    let console = Modes {
        console: true,
        ..Modes::default()
    };
    let mount = "mount -t vfat -o rw,noatime /dev/vda1 /run/zbm-saved-logs";

    startup(&fake, "zbm.timeout=0").run(console, Some(1));
    assert!(!has(&fake, mount));
    fake.files
        .file("/zfsbootmenu/zbm.cmdline", "zbm.timeout=0\nzbm.save_logs\n");
    startup(&fake, "zbm.timeout=0").run(console, Some(1));

    let lines = fake.runner.lines();
    let saved = lines
        .iter()
        .position(|line| line == mount)
        .expect("the logs are saved");
    let execute = lines
        .iter()
        .rposition(|line| line == "kexec -e -i")
        .unwrap();
    assert!(saved < execute, "{lines:?}");
    let saved = "/run/zbm-saved-logs/EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z/kernel.log";
    assert!(zbm_platform::Files::exists(&fake.files, Path::new(saved)));
}

#[test]
fn the_countdown_can_be_interrupted() {
    let fake = machine();
    fake.console.keys(b"\x1b");
    let console = Modes {
        console: true,
        ..Modes::default()
    };
    startup(&fake, "zbm.timeout=5").run(console, Some(1));
    assert!(
        fake.console
            .output_text()
            .contains("Booting \x1b[0;36mtank/ROOT/void\x1b[0m in")
    );
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("kexec"))
    );
    assert!(fake.kmsg().contains(&format!(
        "{}: the menu (a key was pressed during the countdown)",
        support::MESSAGE
    )));
    // The refusal and the loop each open the shell.
    assert_eq!(shells(&fake).len(), 2);
}

#[test]
fn what_the_minimal_runtime_refuses() {
    let console = Modes {
        console: true,
        ..Modes::default()
    };
    // The menu.
    let fake = machine();
    startup(&fake, "zbm.show").run(console, Some(0));
    assert!(fake.kmsg().contains("the menu (zbm.show)"));
    assert_eq!(shells(&fake).len(), 1);
}

#[test]
fn a_key_that_cannot_be_loaded_opens_the_shell() {
    let console = Modes {
        console: true,
        ..Modes::default()
    };
    let fake = machine();
    let runner = &fake.runner;
    runner.respond("zpool list -H -o feature@encryption tank", 0, "active\n");
    runner.respond(
        "zfs get -H -o value encryptionroot tank/ROOT/void",
        0,
        "tank/ROOT\n",
    );
    runner.respond(
        "zfs get -H -o value keystatus tank/ROOT",
        0,
        "unavailable\n",
    );
    runner.respond("zfs get -H -o value keylocation tank/ROOT", 0, "prompt\n");
    runner.respond("zfs load-key -L prompt tank/ROOT", 1, "");
    startup(&fake, "zbm.timeout=0").run(console, Some(0));
    assert!(
        fake.runner
            .lines()
            .iter()
            .any(|line| line == "zfs load-key -L prompt tank/ROOT")
    );
    let banner = "emergency shell\x1b[0m: unable to load key for \x1b[0;36mtank/ROOT/void";
    assert!(fake.console.output_text().contains(banner));
    assert!(!fake.kmsg().contains(support::MESSAGE));
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("mount"))
    );
}

#[test]
fn start_up_and_import_options_run_through() {
    let fake = machine();
    fake.files
        .device("/dev/sda")
        .device("/dev/disk/by-label/hooks");
    fake.runner
        .respond("zpool list -H -o bootfs tank", 0, "tank/ROOT/void\n");
    fake.runner.respond(
        "zfs list -H -o name -t filesystem tank/ROOT/void",
        0,
        "tank/ROOT/void\n",
    );
    let cmdline = concat!(
        "loglevel=7 zbm.wait_for=/dev/sda zbm.import_policy=force ",
        "zbm.prefer=tank/ROOT/void! zbm.hookroot=LABEL=hooks//zbm",
    );
    startup(&fake, cmdline).run(Modes::default(), Some(0));
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert_eq!(
        state.finished_stages(),
        Stage::ALL
            .iter()
            .map(|stage| stage.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(state.bootfs().as_deref(), Some("tank/ROOT/void"));
    for line in [
        "zpool import -N -f -o readonly=on tank",
        "mount -r LABEL=hooks /zfsbootmenu/.external_hooks",
    ] {
        assert!(has(&fake, line), "{line}");
    }
    assert!(shells(&fake).is_empty());
    assert!(!fake.kmsg().contains(support::MESSAGE));
}

#[test]
fn a_restart_goes_to_the_recovery_loop() {
    let fake = machine_with_health("tank\tDEGRADED\nnew\tONLINE\n");
    let state = State::new(&fake.files, "/zfsbootmenu");
    fake.files.dir("/zfsbootmenu");
    for stage in Stage::ALL {
        state.finish_stage(stage.as_str()).unwrap();
    }
    state.set_active(77).unwrap();
    let resume = Modes {
        console: true,
        resume: true,
        ..Modes::default()
    };
    startup(&fake, "zbm.timeout=0").run(resume, Some(2));
    assert!(!has(&fake, "modprobe zfs"));
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("kexec"))
    );
    // The takeover shell, then one per round.
    assert_eq!(shells(&fake).len(), 3);
    assert!(
        fake.console
            .output_text()
            .contains("an active instance is already running")
    );
    assert_eq!(
        fake.runner
            .lines()
            .iter()
            .filter(|line| *line == "zpool import -N -o readonly=on -a")
            .count(),
        2
    );
    // Unhealthy pools are restricted once.
    assert_eq!(state.degraded_pools(), ["tank"]);
}

#[test]
fn luks_volumes_are_unlocked_by_zluks() {
    let fake = machine();
    fake.environment.set("PATH", "/sbin");
    fake.files.file_with_mode("/sbin/cryptsetup", "", 0o755);
    startup(&fake, "zbm.luks=UUID=1234").run(Modes::default(), Some(0));
    let calls = fake.runner.calls();
    let zluks = calls
        .iter()
        .find(|call| call.program() == "zluks")
        .expect("zluks ran");
    assert_eq!(zluks.display(), "zluks unlock");
    assert_eq!(*zluks.get_stdin(), Input::Inherit);
    assert!(zluks.get_env().is_empty());
    // After the stage, before the import.
    let lines = fake.runner.lines();
    let unlock = lines
        .iter()
        .position(|line| line == "zluks unlock")
        .unwrap();
    assert_eq!(lines[unlock + 1], "tput clear");
    assert!(
        lines[unlock..]
            .iter()
            .any(|line| line.starts_with("zpool import"))
    );
    assert!(!fake.kmsg().contains(support::MESSAGE));

    // Without cryptsetup there is nothing to unlock.
    let fake = machine();
    startup(&fake, "quiet").run(Modes::default(), Some(0));
    assert!(!has(&fake, "zluks unlock"));
}

#[test]
fn late_luks_volumes_are_unlocked_as_retries() {
    let fake = machine();
    fake.environment.set("PATH", "/sbin");
    fake.files.file_with_mode("/sbin/cryptsetup", "", 0o755);
    let state = State::new(&fake.files, "/zfsbootmenu");
    let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
    fake.runner.respond("zluks unlock", 1, "");
    let mut shell = Shell::new(fake.system(), &state);
    assert_eq!(shell.unlock_late(), Err(BootError::Zluks { status: 1 }));
    let retry = fake.runner.calls().pop().unwrap();
    assert_eq!(retry.display(), "zluks unlock");
    assert_eq!(retry.get_env(), [("LUKS_RETRY".into(), Some("1".into()))]);
    let fake = machine();
    let mut shell = Shell::new(fake.system(), &state);
    assert_eq!(shell.unlock_late(), Err(BootError::NoCryptsetup));
    assert!(fake.runner.calls().is_empty());
}
