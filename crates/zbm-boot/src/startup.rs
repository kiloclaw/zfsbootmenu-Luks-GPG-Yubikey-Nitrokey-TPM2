//! The start-up program: `libexec/zfsbootmenu-init` and `emergency_shell`.
//!
//! [`Startup::run`] runs the start-up stages that have not run (all of them
//! when forced), marks the start-up done, and on the console counts down and
//! boots the default boot environment. The Phase 5 runtime has no menu: where
//! the legacy program starts the menu, it says what it cannot do and opens the
//! recovery shell, and it keeps doing so, importing pools again between shells
//! as the legacy loop does. See "Modes of the start-up program" and "The
//! minimal runtime of Phase 5" in `crates/DESIGN.md`.

use std::path::{Path, PathBuf};

use zbm_cmdline::args::ZbmArgs;
use zbm_cmdline::conf::Conf;
use zbm_cmdline::options::ZbmOptions;
use zbm_core::exec::{Command, Input, Signal, Stream};
use zbm_core::shell::{read_lines, test_integer};
use zbm_log::read::read_logs;
use zbm_state::State;
use zbm_system::files::FileKind;
use zbm_system::{KernelLog, System};

use crate::boot::{self, BootSettings};
use crate::import::{self, Import, Settings};
use crate::kernels::Kernel;
use crate::prompt::{Answer, TimedPrompt, colorize, tput};
use crate::recovery::Recovery;
use crate::support::{self, Facts, LuksMode, Unsupported};
use crate::{devices, hooks, hostid, modules};

/// The start-up stages, in order, with the names of the legacy scripts.
pub const STAGES: [&str; 6] = [
    "10-kmods",
    "20-hostid",
    "30-device-wait",
    "40-early-hooks",
    "45-luks-unlock",
    "50-import-pools",
];

/// Whether a variable such as `ZFSBOOTMENU_FORCE_INIT` is on: `yes`, `y`,
/// `true`, `t` or `1`, in any case.
pub fn is_on(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "yes" | "y" | "true" | "t" | "1"
    )
}

/// How the start-up program runs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modes {
    /// `ZFSBOOTMENU_CONSOLE`: on the console, with the countdown and the
    /// recovery loop; otherwise only the stages run.
    pub console: bool,
    /// `ZFSBOOTMENU_FORCE_INIT`: run every stage again.
    pub force: bool,
    /// `--resume`: the supervisor restarted the program after it ended
    /// unexpectedly, so the countdown and the automatic boot are skipped.
    pub resume: bool,
}

/// Whether `name` is an executable file in a directory of `PATH`, as
/// `command -v` finds it.
pub fn command_exists(system: &System<'_>, name: &str) -> bool {
    let path = system.environment.var("PATH").unwrap_or_default();
    path.split(':').filter(|dir| !dir.is_empty()).any(|dir| {
        system
            .files
            .metadata(&Path::new(dir).join(name))
            .is_ok_and(|metadata| metadata.kind == FileKind::File && metadata.is_executable())
    })
}

/// The LUKS volumes on the machine, as `luks_discover_devices` lists them.
pub fn luks_volumes(system: &System<'_>) -> Vec<String> {
    let blkid = Command::new("blkid")
        .args(["-c", "/dev/null", "-o", "device", "-t", "TYPE=crypto_LUKS"])
        .stderr(Stream::Null);
    let output = system
        .runner
        .run(&blkid)
        .map(|output| output.stdout_text())
        .unwrap_or_default();
    let mut volumes: Vec<String> = read_lines(&format!(
        "{}\n",
        zbm_core::shell::command_substitution(&output)
    ))
    .into_iter()
    .filter(|device| {
        system
            .files
            .metadata(Path::new(device))
            .is_ok_and(|metadata| metadata.kind == FileKind::Other)
    })
    .map(|device| {
        system
            .files
            .canonicalize(Path::new(&device))
            .map_or(device, |path| path.to_string_lossy().into_owned())
    })
    .collect();
    volumes.sort();
    volumes.dedup();
    volumes
}

/// The recovery shell and late LUKS volumes of the start-up program.
pub struct Shell<'s, 'a> {
    system: System<'a>,
    state: &'s State<'a>,
    log: &'s KernelLog<'a>,
    luks: LuksMode,
}

impl<'s, 'a> Shell<'s, 'a> {
    /// The recovery shell of `system`; `luks` decides whether LUKS volumes
    /// that appear late are looked for.
    pub fn new(
        system: System<'a>,
        state: &'s State<'a>,
        log: &'s KernelLog<'a>,
        luks: LuksMode,
    ) -> Shell<'s, 'a> {
        Shell {
            system,
            state,
            log,
            luks,
        }
    }

    /// Logs each thing the Phase 5 runtime cannot handle as an error and opens
    /// the recovery shell, if there is any. Returns whether there was.
    pub fn refuse(&mut self, list: &[Unsupported]) -> bool {
        if list.is_empty() {
            return false;
        }
        for item in list {
            self.log.error(format!("{}: {item}", support::MESSAGE));
        }
        self.emergency_shell(support::MESSAGE);
        true
    }
}

impl Recovery for Shell<'_, '_> {
    /// LUKS volumes cannot be unlocked yet: volumes that appear late are
    /// reported, and none is opened.
    fn unlock_late(&mut self) -> bool {
        if self.luks == LuksMode::Auto {
            let volumes = luks_volumes(&self.system);
            if !volumes.is_empty() {
                self.refuse(&[Unsupported::LuksVolumes(volumes)]);
            }
        }
        false
    }

    /// `emergency_shell`: a banner with the message and the errors logged since
    /// the last shell, then an interactive Bash with the default `SIGINT`
    /// action; afterwards, ZFS file systems mounted in it are unmounted.
    fn emergency_shell(&mut self, message: &str) {
        let system = &self.system;
        tput(system, &["clear"]);
        tput(system, &["cnorm"]);
        let stty = Command::new("stty")
            .arg("echo")
            .stdin(Input::Inherit)
            .stdout(Stream::Inherit);
        let _ = system.runner.run(&stty);

        let suffix = if message.is_empty() {
            String::new()
        } else {
            format!(": {message}")
        };
        let banner = format!(
            "{}{suffix}\n\ntype '{}' for online documentation\n\
             type '{}' to return to ZFSBootMenu\n\n",
            colorize("green", "emergency shell"),
            colorize("red", "help"),
            colorize("red", "exit"),
        );
        let _ = system.console.write(banner.as_bytes());
        if self.state.has_errors() {
            if let Ok(errors) = read_logs(system.runner, "err", None) {
                let _ = system.console.write(errors.as_bytes());
            }
            let _ = system.console.write(b"\n");
            self.state.clear_errors();
        }
        if command_exists(system, "efibootmgr") {
            boot::mount_efivarfs(system, self.log, "rw");
        }

        let shell = Command::new("/bin/bash")
            .arg("-i")
            .stdin(Input::Inherit)
            .stdout(Stream::Inherit)
            .stderr(Stream::Inherit)
            .default_signal(Signal::Interrupt);
        let _ = system.runner.run(&shell);

        let mounts = system
            .files
            .read_to_string(Path::new("/proc/self/mounts"))
            .unwrap_or_default();
        for line in read_lines(&mounts) {
            let mut fields = line.split_whitespace();
            let (Some(mountpoint), Some("zfs")) = (fields.nth(1), fields.next()) else {
                continue;
            };
            let umount = Command::new("umount")
                .arg(mountpoint)
                .stdout(Stream::Inherit);
            let _ = system.runner.run(&umount);
            self.log.debug(format!("unmounting: {mountpoint}"));
        }
        boot::mount_efivarfs(system, self.log, "ro");
    }
}

/// The start-up program, with what it read at start.
pub struct Startup<'a> {
    system: System<'a>,
    state: State<'a>,
    log: KernelLog<'a>,
    options: ZbmOptions,
    args: ZbmArgs,
    release_build: bool,
    hooks: PathBuf,
    luks: LuksMode,
}

impl<'a> Startup<'a> {
    /// The start-up program for `system` with the configuration `conf`, which
    /// also names the state directory.
    pub fn new(system: System<'a>, conf: &Conf) -> Startup<'a> {
        let options = ZbmOptions::from_conf(conf);
        let base = crate::preinit::base(conf);
        let state = State::new(system.files, &base);
        let log = system.kernel_log(&options.loglevel, &base);
        let kcl = system
            .files
            .read_to_string(&base.join("zbm.cmdline"))
            .unwrap_or_default();
        let args = ZbmArgs::from_kcl(&kcl);
        let cryptsetup = command_exists(&system, "cryptsetup");
        let crypttab = system.files.read(Path::new(support::CRYPTTAB)).is_ok();
        let luks = LuksMode::decide(args.get("zbm.luks"), crypttab, cryptsetup);
        Startup {
            system,
            state,
            log,
            options,
            args,
            release_build: !conf.get("ZBM_RELEASE_BUILD").unwrap_or_default().is_empty(),
            hooks: PathBuf::from(hooks::HOOKS),
            luks,
        }
    }

    /// The same program with hooks somewhere else, for tests.
    pub fn with_hooks(mut self, hooks: &Path) -> Startup<'a> {
        hooks.clone_into(&mut self.hooks);
        self
    }

    fn shell(&self) -> Shell<'_, 'a> {
        Shell::new(self.system, &self.state, &self.log, self.luks)
    }

    fn import_settings(&self) -> Settings {
        Settings {
            import_policy: self.options.import_policy.clone(),
            prefer_pool: self.options.prefer_pool.clone(),
            require_pool: self.options.require_pool.clone(),
            prefer_bootfs: self.options.prefer_bootfs.clone(),
            retry_delay: self.options.retry_delay.clone(),
            release_build: self.release_build,
        }
    }

    fn facts(&self) -> Facts {
        Facts {
            luks: Some(self.luks),
            load_key_hooks: hooks::executable_hooks(self.system.files, &self.hooks, "load-key.d"),
            ..Facts::default()
        }
    }

    /// Runs the program. Without the console mode it returns after the stages;
    /// otherwise it returns only after `rounds` rounds of the recovery loop
    /// (never, with `None`).
    pub fn run(&self, modes: Modes, rounds: Option<usize>) -> i32 {
        let _ = self.state.create();
        boot::mount_efivarfs(&self.system, &self.log, "ro");

        let pending = STAGES.iter().any(|stage| {
            modes.force
                || !self
                    .state
                    .finished_stages()
                    .iter()
                    .any(|done| done == stage)
        });
        let refused = pending
            && self
                .shell()
                .refuse(&support::before_start(&self.options, &self.facts()));
        let mut bootfs = None;
        if !refused {
            bootfs = self.run_stages(modes.force);
            if let Err(error) = self.state.mark_initialized() {
                self.log.error(error.to_string());
            }
        }
        if !modes.console {
            return 0;
        }
        let bootfs = bootfs.or_else(|| self.state.bootfs());
        if !refused && !modes.resume {
            self.countdown(bootfs.as_deref());
        }
        if self.state.is_active() {
            self.shell()
                .emergency_shell("an active instance is already running");
        }
        self.recovery_loop(rounds);
        0
    }

    /// Runs each stage that has not finished, or all of them when forced, and
    /// returns the default boot environment the import found.
    fn run_stages(&self, force: bool) -> Option<String> {
        let system = &self.system;
        let log = &self.log;
        let mut bootfs = None;
        for stage in STAGES {
            if !force
                && self
                    .state
                    .finished_stages()
                    .iter()
                    .any(|done| done == stage)
            {
                continue;
            }
            log.info(format!("running init stage {stage}"));
            let mut shell = self.shell();
            match stage {
                "10-kmods" => modules::stage(system.runner, log, &mut shell),
                "20-hostid" => hostid::stage(system.files, log, &self.options.spl_hostid),
                "30-device-wait" => devices::stage(
                    system,
                    log,
                    &self.options.wait_for_devices,
                    &self.options.retry_delay,
                ),
                "40-early-hooks" => {
                    let mut messages = Vec::new();
                    let skip = self.args.lookup(&["zbm.skip_hooks"], &mut messages);
                    hooks::early_stage_in(
                        system,
                        log,
                        &self.state,
                        &self.hooks,
                        &self.options.hook_root,
                        skip.as_deref(),
                    );
                }
                "45-luks-unlock" => {
                    if self.luks == LuksMode::Auto {
                        let volumes = luks_volumes(system);
                        if !volumes.is_empty() {
                            shell.refuse(&[Unsupported::LuksVolumes(volumes)]);
                        }
                    }
                }
                _ => {
                    let settings = self.import_settings();
                    let imported = import::stage(system, &self.state, log, &settings, &mut shell);
                    bootfs = imported.bootfs;
                }
            }
            if let Err(error) = self.state.finish_stage(stage) {
                log.error(error.to_string());
            }
        }
        bootfs
    }

    /// The countdown and the automatic boot of `bootfs`.
    fn countdown(&self, bootfs: Option<&str>) {
        let system = &self.system;
        let mut facts = self.facts();
        facts.bootfs = bootfs.map(str::to_owned);
        let timeout = test_integer(&self.options.menu_timeout);
        let Some(bootfs) = bootfs.filter(|_| timeout.is_some_and(|timeout| timeout >= 0)) else {
            self.shell()
                .refuse(&support::before_boot(&self.options, &facts));
            return;
        };

        let width = self.options.menu_timeout.len();
        let name = colorize("cyan", bootfs);
        let answer = TimedPrompt::new()
            .delay(timeout.unwrap_or_default())
            .prompt(move |seconds| {
                let seconds = colorize("yellow", &format!("{seconds:0width$}"));
                format!("Booting {name} in {seconds} seconds")
            })
            .on_return("boot now ")
            .on_escape("boot menu")
            .run(system);
        if answer == Answer::Escape {
            facts.countdown_interrupted = true;
            self.shell()
                .refuse(&support::before_boot(&self.options, &facts));
            return;
        }
        // Someone took over over SSH: do not boot.
        if self.state.is_active() {
            return;
        }
        tput(system, &["clear"]);
        // load_key: keys are not loaded yet.
        self.log.debug(format!("fs set to {bootfs}"));
        facts.locked_encryption_root = boot::locked_root(system.runner, &self.log, bootfs);
        if self
            .shell()
            .refuse(&support::before_boot(&self.options, &facts))
        {
            return;
        }
        if boot::find_kernels(system, &self.state, &self.log, bootfs) && !self.state.is_active() {
            let selected = boot::select_kernel(system, &self.state, &self.log, bootfs)
                .unwrap_or_else(|| Kernel::parse(""));
            // 50-import-pools exported BOOTFS in the legacy program.
            let settings = BootSettings {
                set_hostid: self.options.set_hostid,
                hooks: self.hooks.clone(),
                env: vec![("BOOTFS".to_owned(), bootfs.to_owned())],
            };
            let mut shell = self.shell();
            boot::kexec_kernel(
                system,
                &self.state,
                &self.log,
                &settings,
                &selected,
                &mut shell,
            );
        }
    }

    /// The loop of the legacy program, without the menu: the recovery shell,
    /// then pools that became importable in it are imported read-only, and
    /// unhealthy ones restricted.
    fn recovery_loop(&self, rounds: Option<usize>) {
        let mut round = 0;
        while rounds.is_none_or(|rounds| round < rounds) {
            round += 1;
            self.shell().emergency_shell(support::MESSAGE);

            let pool = if self.options.require_pool == "only" {
                self.options.prefer_pool.as_str()
            } else {
                ""
            };
            let reimport = Import {
                pool,
                force: self.options.import_policy == "force",
                ..Import::default()
            };
            import::import_pool(&self.system, &self.state, &self.log, reimport);
            let list = Command::new("zpool")
                .args(["list", "-H", "-o", "name,health"])
                .stderr(Stream::Null);
            let output = self
                .system
                .runner
                .run(&list)
                .map(|output| output.stdout_text())
                .unwrap_or_default();
            for line in import::tab_lines(&output) {
                let (pool, health) = import::tab_fields(&line);
                if !pool.is_empty() && health != "ONLINE" && !self.state.is_degraded(pool) {
                    let _ = self.state.restrict(pool);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testutil::fakes::FakeSystem;

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
    fn the_shell_banner_matches_the_legacy_one() {
        use zbm_testutil::legacy::{Bash, LOG_STUBS};
        for message in ["", "unable to mount \x1b[0;36mtank\x1b[0m"] {
            let legacy = Bash::new()
                .code(LOG_STUBS)
                .code(
                    r#"source() {
                        case "$1" in
                          /lib/*) builtin source "$LEGACY/zfsbootmenu/lib/${1#/lib/}" ;;
                          *) builtin source "$@" ;;
                        esac
                      }"#,
                )
                .source("zfsbootmenu/lib/zfsbootmenu-core.sh")
                .code("tput() { :; }; stty() { :; }; mount_efivarfs() { :; }")
                .env("BASE", "/nonexistent")
                .call("emergency_shell", &[message], b"exit\n");
            let fake = machine();
            let state = State::new(&fake.files, "/zfsbootmenu");
            let log = KernelLog::new(&fake.files, "4", Path::new("/zfsbootmenu"), 1);
            Shell::new(fake.system(), &state, &log, LuksMode::Unavailable).emergency_shell(message);
            assert_eq!(
                fake.console.output_text(),
                legacy.stdout_text(),
                "{message:?}"
            );
        }
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
        assert_eq!(state.finished_stages(), STAGES);
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
        let order: Vec<usize> = STAGES
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
            force: true,
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
        // An option the stages do not handle yet: no stage runs.
        let fake = machine();
        let console = Modes {
            console: true,
            ..Modes::default()
        };
        startup(&fake, "zbm.wait_for=/dev/sda").run(console, Some(1));
        assert!(!has(&fake, "modprobe zfs"));
        assert!(!State::new(&fake.files, "/zfsbootmenu").is_initialized());
        assert!(fake.kmsg().contains("zbm.wait_for=/dev/sda"));
        assert_eq!(shells(&fake).len(), 2);

        // The menu.
        let fake = machine();
        startup(&fake, "zbm.show").run(console, Some(0));
        assert!(fake.kmsg().contains("the menu (zbm.show)"));
        assert_eq!(shells(&fake).len(), 1);

        // A locked boot environment.
        let fake = machine();
        fake.runner
            .respond("zpool list -H -o feature@encryption tank", 0, "active\n");
        fake.runner.respond(
            "zfs get -H -o value encryptionroot tank/ROOT/void",
            0,
            "tank/ROOT\n",
        );
        fake.runner.respond(
            "zfs get -H -o value keystatus tank/ROOT",
            0,
            "unavailable\n",
        );
        startup(&fake, "zbm.timeout=0").run(console, Some(0));
        assert!(fake.kmsg().contains("the locked encryption root tank/ROOT"));
        assert!(
            !fake
                .runner
                .lines()
                .iter()
                .any(|line| line.starts_with("mount"))
        );
    }

    #[test]
    fn a_restart_goes_to_the_recovery_loop() {
        let fake = machine_with_health("tank\tDEGRADED\nnew\tONLINE\n");
        let state = State::new(&fake.files, "/zfsbootmenu");
        fake.files.dir("/zfsbootmenu");
        for stage in STAGES {
            state.finish_stage(stage).unwrap();
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
    fn late_luks_volumes_are_refused() {
        let fake = machine();
        fake.environment.set("PATH", "/sbin");
        fake.files
            .file_with_mode("/sbin/cryptsetup", "", 0o755)
            .device("/dev/sdb2");
        fake.runner.respond(
            "blkid -c /dev/null -o device -t TYPE=crypto_LUKS",
            0,
            "/dev/sdb2\n",
        );
        startup(&fake, "quiet").run(Modes::default(), Some(0));
        assert!(fake.kmsg().contains("LUKS volumes: /dev/sdb2"));
        assert_eq!(shells(&fake).len(), 1);
    }
}
