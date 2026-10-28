//! The start-up program: `libexec/zfsbootmenu-init`.
//!
//! [`Startup::run`] runs the start-up stages that have not run (all of them
//! when forced), marks the start-up done, and on the console counts down and
//! boots the default boot environment. Then, as the legacy loop does, it runs
//! the menu program ([`menu`]) and the recovery shell after it, importing
//! pools again between rounds. When the image has the menu program, the menu
//! counts down instead, showing the boot environments meanwhile
//! (`zfsbootmenu --countdown`). In an image without it, the program says
//! what it cannot do where the legacy one shows the menu. See "Modes of the
//! start-up program" and "The minimal runtime of Phase 5" in
//! `crates/DESIGN.md`.

pub mod menu;

use std::path::{Path, PathBuf};

use zbm_boot::boot::{self, BootSettings};
use zbm_boot::hooks;
use zbm_boot::import::{self, Import, ImportSettings};
use zbm_boot::kernels::Kernel;
use zbm_boot::keys::{self, BootPath, Clear, KeyLoad};
use zbm_boot::prompt::{Answer, TimedPrompt, colorize, tput};
use zbm_boot::recovery::Recovery;
use zbm_boot::saved_logs::Destination;
use zbm_boot::support::{self, Facts};
use zbm_config::args::{Args, ZbmArgs};
use zbm_config::conf::Conf;
use zbm_config::options::ZbmOptions;
use zbm_config::spec::cmdline::{SAVE_LOGS, SKIP_HOOKS};
use zbm_platform::{Context, Installed, System};
use zbm_pool::Build;
use zbm_state::State;
use zbm_tools::programs;
use zbm_types::{DatasetName, FilesystemName, PoolRequirement};
use zbm_zfs::{Stderr, ZpoolCommand};

use crate::front::{ConsoleFront, Front};
use crate::shell::Shell;
use crate::stages;
use crate::step::{Rerun, Step, StepContext, run_steps};

/// Whether a variable such as `ZFSBOOTMENU_FORCE_INIT` is on: `yes`, `y`,
/// `true`, `t` or `1`, in any case.
pub fn is_on(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "yes" | "y" | "true" | "t" | "1"
    )
}

/// What the first round of the recovery loop does.
enum FirstRound<'b> {
    /// The menu counts down to booting the environment, if any; start-up
    /// counts down itself when the menu cannot start.
    Countdown(Option<&'b DatasetName>),
    /// The menu opens without counting down.
    Menu,
}

/// How the start-up program runs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modes {
    /// `ZFSBOOTMENU_CONSOLE`: on the console, with the countdown and the
    /// recovery loop; otherwise only the stages run.
    pub console: bool,
    /// `ZFSBOOTMENU_FORCE_INIT`: run every stage again.
    pub rerun: Rerun,
    /// `--resume`: the supervisor restarted the program after it ended
    /// unexpectedly, so the countdown and the automatic boot are skipped.
    pub resume: bool,
}

/// The start-up program, with what it read at start.
pub struct Startup<'a> {
    system: System<'a>,
    state: State<'a>,
    /// Keeps this thread's messages going to the kernel log.
    _log: Installed,
    options: ZbmOptions,
    args: ZbmArgs,
    build: Build,
    hooks: PathBuf,
    /// The variables of `/etc/zfsbootmenu.conf`, which the menu's hooks get
    /// even when it was not started from a shell that sourced the file
    /// (`ssh host zfsbootmenu`).
    conf_vars: Vec<(String, String)>,
}

impl<'a> Startup<'a> {
    /// The start-up program for `system` with the configuration `conf`, which
    /// also names the state directory.
    pub fn new(system: System<'a>, conf: &Conf) -> Startup<'a> {
        let options = ZbmOptions::from_conf(conf);
        let base = crate::preinit::base(conf);
        let state = State::new(system.files, &base);
        let log = system.kernel_log(&options.loglevel.to_string(), &base);
        let kcl = system
            .files
            .read_to_string(&base.join("zbm.cmdline"))
            .unwrap_or_default();
        let args = ZbmArgs::from_cmdline(&kcl);
        Startup {
            system,
            state,
            _log: log,
            options,
            args,
            build: if conf.get("ZBM_RELEASE_BUILD").unwrap_or_default().is_empty() {
                Build::Local
            } else {
                Build::Release
            },
            hooks: PathBuf::from(hooks::HOOKS),
            conf_vars: conf.vars().to_vec(),
        }
    }

    /// The same program with hooks somewhere else, for tests.
    #[must_use]
    pub fn with_hooks(mut self, hooks: &Path) -> Startup<'a> {
        hooks.clone_into(&mut self.hooks);
        self
    }

    fn shell(&self) -> Shell<'_, 'a> {
        Shell::new(self.system, &self.state)
    }

    fn import_settings(&self) -> ImportSettings {
        ImportSettings {
            policy: self.options.import_policy,
            prefer_pool: self.options.prefer_pool.clone(),
            requirement: self.options.require_pool,
            prefer_bootfs: self.options.prefer_bootfs.clone(),
            retry_delay: self.options.retry_delay,
            build: self.build,
        }
    }

    /// Where `zbm.save_logs` saves the logs of a boot, if anywhere.
    fn save_logs(&self) -> Option<Destination> {
        let value = self.args.lookup(&SAVE_LOGS, &mut Vec::new());
        Destination::from_option(value.as_deref())
    }

    /// Runs the program. Without the console mode it returns after the stages;
    /// otherwise it returns only after `rounds` rounds of the recovery loop
    /// (never, with `None`).
    pub fn run(&self, modes: Modes, rounds: Option<usize>) -> i32 {
        self.run_with(modes, rounds, None)
    }

    /// [`Startup::run`], with the stages on `stages` when given: their
    /// programs on its system, and what they ask through its front end,
    /// such as the menu's start-up view, which they finish with. Otherwise
    /// they run on the plain console.
    pub fn run_with(
        &self,
        modes: Modes,
        rounds: Option<usize>,
        stages: Option<(&System<'_>, &mut dyn Front)>,
    ) -> i32 {
        let _ = self.state.create();
        boot::mount_efivarfs(&self.system, "ro");

        let bootfs = match stages {
            Some((system, front)) => {
                let bootfs = self.run_stages_on(system, front, modes.rerun);
                front.finish();
                bootfs
            }
            None => self.run_stages(modes.rerun),
        };
        if let Err(error) = self.state.mark_initialized() {
            zbm_trace::error!("{}", error.to_string());
        }
        if !modes.console {
            return 0;
        }
        // The state directory's copy is what an earlier start found.
        let bootfs = bootfs.or_else(|| self.state.bootfs().and_then(|name| name.parse().ok()));
        // The menu counts down itself, showing the environments meanwhile.
        let has_menu = menu::has_menu(&self.system);
        if !modes.resume && !has_menu {
            self.countdown(bootfs.as_ref());
        }
        // Legacy never boots by itself once it saw another instance.
        let taken = self.state.is_active();
        if taken {
            self.shell()
                .emergency_shell("an active instance is already running");
        }
        let first = if modes.resume || taken {
            FirstRound::Menu
        } else {
            FirstRound::Countdown(bootfs.as_ref())
        };
        self.recovery_loop(rounds, &first);
        0
    }

    /// The start-up stages, in the order they run.
    pub fn steps(&self) -> Vec<Box<dyn Step + '_>> {
        let mut messages = Vec::new();
        let skip = self.args.lookup(&SKIP_HOOKS, &mut messages);
        vec![
            Box::new(stages::Modules),
            Box::new(stages::HostId {
                spl_hostid: self.options.spl_hostid.as_ref(),
            }),
            Box::new(stages::DeviceWait {
                wait_for: &self.options.wait_for_devices,
                retry_delay: self.options.retry_delay,
            }),
            Box::new(stages::EarlyHooks {
                root: &self.hooks,
                hook_root: &self.options.hook_root,
                skip,
            }),
            Box::new(stages::LuksUnlock),
            Box::new(stages::ImportPools {
                settings: self.import_settings(),
            }),
        ]
    }

    /// Runs the stages that `rerun` selects on the plain console, and
    /// returns the default boot environment the import found.
    fn run_stages(&self, rerun: Rerun) -> Option<DatasetName> {
        let mut front = ConsoleFront::new(self.system, &self.state);
        self.run_stages_on(&self.system, &mut front, rerun)
    }

    /// Runs the stages that `rerun` selects with `front`, their programs on
    /// `system`, and returns the default boot environment the import found.
    fn run_stages_on(
        &self,
        system: &System<'_>,
        front: &mut dyn Front,
        rerun: Rerun,
    ) -> Option<DatasetName> {
        let mut context = StepContext::new(system, &self.state, front);
        run_steps(&self.steps(), &mut context, rerun);
        context.bootfs
    }

    /// The countdown and the automatic boot of `bootfs`. Where the legacy
    /// program would show the menu, the loop that follows does; without the
    /// menu program, this logs why it cannot.
    fn countdown(&self, bootfs: Option<&DatasetName>) {
        let mut facts = Facts {
            bootfs: bootfs.map(DatasetName::to_string),
            has_menu: menu::has_menu(&self.system),
            ..Facts::default()
        };
        let timeout = self.options.menu_timeout.seconds();
        let (Some(bootfs), Some(seconds)) = (bootfs, timeout) else {
            log_unsupported(&support::unsupported(&self.options, &facts));
            return;
        };

        if self.count_down(bootfs, seconds) == Answer::Escape {
            facts.countdown_interrupted = true;
            log_unsupported(&support::unsupported(&self.options, &facts));
            return;
        }
        // Someone took over over SSH: do not boot.
        if self.state.is_active() {
            return;
        }
        self.boot(bootfs);
    }

    /// Counts down `seconds` before booting `bootfs`: RETURN boots at once,
    /// ESCAPE stops.
    fn count_down(&self, bootfs: &DatasetName, seconds: u64) -> Answer {
        let width = self.options.menu_timeout.to_string().len();
        let name = colorize("cyan", bootfs.as_str());
        TimedPrompt::new()
            .delay(i64::try_from(seconds).unwrap_or(i64::MAX))
            .prompt(move |seconds| {
                let seconds = colorize("yellow", &format!("{seconds:0width$}"));
                format!("Booting {name} in {seconds} seconds")
            })
            .on_return("boot now ")
            .on_escape("boot menu")
            .run(&self.system)
    }

    /// Loads the key of `bootfs` and boots it with its selected kernel, or
    /// opens the recovery shell when the key cannot be loaded.
    fn boot(&self, bootfs: &DatasetName) {
        let system = &self.system;
        // Clear the screen before a possible passphrase prompt.
        tput(system, &["clear"]);
        // `NO_CACHE=1 load_key`: the countdown tries no key sources.
        let how = KeyLoad {
            hooks: &self.hooks,
            path: BootPath::Countdown,
            clear: Clear::No,
        };
        if keys::load_key(system, &self.state, &how, bootfs).is_err() {
            let message = format!(
                "unable to load key for {}",
                colorize("cyan", bootfs.as_str())
            );
            self.shell().emergency_shell(&message);
            return;
        }
        let filesystem = FilesystemName::from(bootfs.clone());
        if boot::find_kernels(system, &self.state, &filesystem).is_ok() && !self.state.is_active() {
            let selected = boot::select_kernel(system, &self.state, &filesystem)
                .unwrap_or_else(|| Kernel::parse(""));
            // 50-import-pools exported BOOTFS in the legacy program.
            let settings = BootSettings {
                set_hostid: self.options.set_hostid,
                hooks: self.hooks.clone(),
                env: vec![("BOOTFS".to_owned(), bootfs.to_string())],
                save_logs: self.save_logs(),
                command_line: None,
                import_policy: self.options.import_policy,
            };
            let mut shell = self.shell();
            boot::kexec_kernel(system, &self.state, &settings, &selected, &mut shell);
        }
    }

    /// The loop of the legacy program: the menu program, as `first` says
    /// the first time, and the recovery shell, then pools that became
    /// importable in them are imported read-only, and unhealthy ones
    /// restricted. Without the menu program, the shell says that the menu
    /// is not supported.
    fn recovery_loop(&self, rounds: Option<usize>, first: &FirstRound<'_>) {
        let mut round = 0;
        while rounds.is_none_or(|rounds| round < rounds) {
            round += 1;
            if menu::has_menu(&self.system) {
                let countdown = match first {
                    FirstRound::Countdown(bootfs) if round == 1 => Some(*bootfs),
                    _ => None,
                };
                let args: &[&str] = if countdown.is_some() {
                    &["--countdown"]
                } else {
                    &[]
                };
                let ran = programs::applet(self.system.runner, menu::MENU, args, &[]);
                // Anything but a clean end is a failure: the menu could not
                // read the machine or use the terminal (`menu::FAILED`), it
                // panicked, a signal killed it, or it did not start.
                let failed = !ran.is_ok_and(|output| output.status.is_success());
                if let Some(bootfs) = countdown.filter(|_| failed) {
                    // The automatic boot must not depend on the menu.
                    zbm_trace::warn!("the menu could not start; counting down without it");
                    self.countdown(bootfs);
                }
                self.shell().emergency_shell("");
            } else {
                self.shell().emergency_shell(support::MESSAGE);
            }

            let settings = self.import_settings();
            let pool = settings
                .prefer_pool
                .as_ref()
                .filter(|_| settings.requirement == PoolRequirement::Only);
            let reimport = Import::target(pool).policy(settings.policy);
            let context = Context::new(self.system);
            let _ = import::import_pool(&context, &self.state, reimport);
            let output = ZpoolCommand::new(self.system.runner)
                .text(&["list", "-H", "-o", "name,health"], Stderr::Hidden)
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

/// Logs each item of `list` as an error.
fn log_unsupported(list: &[support::Unsupported]) {
    for item in list {
        zbm_trace::error!("{}: {item}", support::MESSAGE);
    }
}

#[cfg(test)]
mod tests;
