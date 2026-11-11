use super::*;
use zbm_boot::BootError;
use zbm_platform::KernelLog;
use zbm_platform::console::Screen;
use zbm_platform::exec::{Input, Signal};
use zbm_testkit::fakes::FakeSystem;
use zbm_types::Stage;

/// The configuration the start-up would write for `cmdline`.
fn conf(cmdline: &str) -> Conf {
    let (options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(cmdline));
    Conf::parse(&options.conf("/zfsbootmenu"))
}

/// A machine with one pool and one boot environment with a kernel.
fn machine() -> FakeSystem {
    machine_with_health("tank\tONLINE\n")
}

/// The machine, with `zpool list -H -o name,health` giving `health`.
fn machine_with_health(health: &str) -> FakeSystem {
    let fake = FakeSystem::new();
    let runner = &fake.runner;
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
    assert!(fake.console.screen_count(Screen::Clear) > 0);
    for line in [
        "modprobe spl",
        "modprobe zfs",
        "udevadm settle",
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

/// ADR 0017, point 7: the countdown's hooks get the configuration's
/// variables and `BOOTFS`, as the menu's do.
#[test]
fn the_countdown_s_hooks_get_the_configuration_and_bootfs() {
    let fake = machine();
    fake.files
        .file_with_mode("/libexec/hooks/boot-sel.d/10-sel", "#!/bin/sh\n", 0o755)
        .file_with_mode("/libexec/hooks/teardown.d/10-down", "#!/bin/sh\n", 0o755)
        .file_with_mode("/usr/bin/cryptsetup", "", 0o755);
    fake.environment.set("PATH", "/usr/bin");
    let console = Modes {
        console: true,
        ..Modes::default()
    };
    startup(&fake, "zbm.timeout=0").run(console, Some(1));
    for hook in [
        "/libexec/hooks/boot-sel.d/10-sel",
        "/libexec/hooks/teardown.d/10-down",
    ] {
        let call = fake
            .runner
            .calls()
            .into_iter()
            .find(|call| call.program() == hook)
            .unwrap_or_else(|| panic!("{hook} ran"));
        let env = call.get_env();
        let has = |name: &str, value: &str| {
            env.iter()
                .any(|(key, set)| key == name && set.as_deref() == Some(value.as_ref()))
        };
        assert!(has("BOOTFS", "tank/ROOT/void"), "{hook}: {env:?}");
        assert!(has("BASE", "/zfsbootmenu"), "{hook}: {env:?}");
        assert!(has("ZBM_SELECTED_BE", "tank/ROOT/void"), "{hook}: {env:?}");
    }
    // The countdown's boot carries the built-in steps: the LUKS close runs.
    assert!(has(&fake, "/bin/zluks close"));
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
    // The first run's `kexec -e` returned, as a fake one does: its message
    // stayed its 10 seconds before the second run saved the logs.
    let saved = "/run/zbm-saved-logs/EFI/zfsbootmenu/logs/2026-01-01T00-00-10Z/kernel.log";
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
    // The refusal is logged; only the loop opens the shell, so that leaving
    // it imports the pools again before the next one.
    assert_eq!(shells(&fake).len(), 1);
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
    assert_eq!(shells(&fake).len(), 0);
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
    // Three wrong keys typed at the prompt.
    fake.console
        .secret(b"wrong1")
        .secret(b"wrong2")
        .secret(b"wrong3");
    startup(&fake, "zbm.timeout=0").run(console, Some(0));
    let tries = fake
        .runner
        .lines()
        .iter()
        .filter(|line| *line == "zfs load-key -L prompt tank/ROOT")
        .count();
    assert_eq!(tries, 3);
    assert!(
        fake.console
            .output_text()
            .contains("Enter passphrase for 'tank/ROOT':")
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
    let _other = state.take_active(77).unwrap().unwrap();
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
    // After the stage, the screen is cleared, before the import.
    let lines = fake.runner.lines();
    let unlock = lines
        .iter()
        .position(|line| line == "zluks unlock")
        .unwrap();
    assert!(fake.console.screen_count(Screen::Clear) > 0);
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
    let shell = Shell::new(fake.system(), &state);
    assert_eq!(shell.unlock_late(), Err(BootError::Zluks { status: 1 }));
    let retry = fake.runner.calls().pop().unwrap();
    assert_eq!(retry.display(), "zluks unlock");
    assert_eq!(retry.get_env(), [("LUKS_RETRY".into(), Some("1".into()))]);
    let fake = machine();
    let shell = Shell::new(fake.system(), &state);
    assert_eq!(shell.unlock_late(), Err(BootError::NoCryptsetup));
    assert!(fake.runner.calls().is_empty());
}

#[test]
fn a_chroot_outside_the_menu_gets_the_menu_s_settings() {
    let fake = machine();
    let conf = Conf::parse(
        "export BASE=\"/zfsbootmenu\"\nexport zbm_set_hostid=\"1\"\n\
         export import_policy=\"force\"\n",
    );

    let settings = Startup::new(fake.system(), &conf).menu_settings();

    assert!(settings.set_hostid);
    assert_eq!(settings.hooks, Path::new(hooks::HOOKS));
    assert_eq!(
        settings.import_policy,
        zbm_types::ImportPolicy::Force,
        "the options of the configuration"
    );
    let env: Vec<(&str, &str)> = settings.env.iter().collect();
    assert!(env.contains(&("zbm_set_hostid", "1")), "{env:?}");
    assert!(
        !env.iter().any(|(name, _)| *name == "BOOTFS"),
        "the key's load sets BOOTFS as it is then"
    );
}

/// The recovery of a boot by files: what it was told.
#[derive(Default)]
struct Recovered(std::sync::Mutex<Vec<String>>);

impl Recovery for Recovered {
    fn unlock_late(&self) -> Result<(), BootError> {
        Err(BootError::NoCryptsetup)
    }

    fn emergency_shell(&self, message: &str) {
        self.0.lock().unwrap().push(message.to_owned());
    }
}

/// The kernel `zkexec tank/ROOT/void vmlinuz-6.1 initramfs-6.1.img` names.
fn by_files() -> Kernel {
    Kernel::new(
        "tank/ROOT/void",
        "/boot/vmlinuz-6.1",
        "/boot/initramfs-6.1.img",
    )
}

#[test]
fn a_kernel_by_its_files_boots_with_the_menu_s_settings_and_says_why_it_did_not() {
    let fake = machine();
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    fake.files
        .file_with_mode("/hooks/boot-sel.d/10-sel", "#!/bin/sh\n", 0o755);
    let told = Recovered::default();

    let status = startup(&fake, "loglevel=7")
        .with_hooks(Path::new("/hooks"))
        .kexec_files(&by_files(), &told);

    assert_eq!(status, menu::FAILED);
    let ran = fake.runner.lines();
    assert!(
        ran.iter().any(|line| line == "/hooks/boot-sel.d/10-sel"),
        "{ran:#?}"
    );
    let load = ran
        .iter()
        .find(|line| line.starts_with("kexec -a -l"))
        .unwrap_or_else(|| panic!("{ran:#?}"));
    assert!(
        load.contains("/zfsbootmenu/environments/tank/ROOT/void/mnt/boot/vmlinuz-6.1"),
        "{load}"
    );
    // The fake's kexec -e returns: the recovery is told why.
    assert_eq!(
        *told.0.lock().unwrap(),
        ["unable to boot tank/ROOT/void: the kernel could not be executed"]
    );
}

/// A boot that told the recovery why it failed, such as a boot
/// environment that cannot be mounted, is not said to have failed a
/// second time, for a kernel that was never loaded.
#[test]
fn a_kernel_by_its_files_is_said_to_have_failed_once() {
    let fake = machine();
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    fake.runner.respond_program("mount", 32, "");
    let told = Recovered::default();

    let status = startup(&fake, "loglevel=7").kexec_files(&by_files(), &told);

    assert_eq!(status, menu::FAILED);
    let said = told.0.lock().unwrap().clone();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("unable to mount "), "{said:?}");

    // What it does not tell itself goes to the recovery as it is.
    let told = Recovered::default();
    let telling = Told {
        recovery: &told,
        told: std::sync::atomic::AtomicBool::new(false),
    };
    assert!(matches!(
        telling.unlock_late(),
        Err(BootError::NoCryptsetup)
    ));
    assert!(!telling.is_told());
}
