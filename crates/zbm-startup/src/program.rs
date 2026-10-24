//! The start-up program: `libexec/zfsbootmenu-init`.
//!
//! [`Startup::run`] runs the start-up stages that have not run (all of them
//! when forced), marks the start-up done, and on the console counts down and
//! boots the default boot environment. The Phase 5 runtime has no menu: where
//! the legacy program starts the menu, it says what it cannot do and opens the
//! recovery shell, and it keeps doing so, importing pools again between shells
//! as the legacy loop does. See "Modes of the start-up program" and "The
//! minimal runtime of Phase 5" in `crates/DESIGN.md`.

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
use zbm_platform::{Installed, System};
use zbm_pool::{BootfsPreference, Build, PoolRef, Requirement, RetryDelay};
use zbm_state::State;
use zbm_zfs::{Stderr, ZpoolCommand};

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
            prefer_pool: PoolRef::new(&self.options.prefer_pool),
            requirement: Requirement::from_conf(&self.options.require_pool),
            prefer_bootfs: BootfsPreference::from_conf(&self.options.prefer_bootfs),
            retry_delay: RetryDelay::from_conf(&self.options.retry_delay),
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
        let _ = self.state.create();
        boot::mount_efivarfs(&self.system, "ro");

        let bootfs = self.run_stages(modes.rerun);
        if let Err(error) = self.state.mark_initialized() {
            zbm_trace::error!("{}", error.to_string());
        }
        if !modes.console {
            return 0;
        }
        let bootfs = bootfs.or_else(|| self.state.bootfs());
        if !modes.resume {
            self.countdown(bootfs.as_deref());
        }
        if self.state.is_active() {
            self.shell()
                .emergency_shell("an active instance is already running");
        }
        self.recovery_loop(rounds);
        0
    }

    /// The start-up stages, in the order they run.
    pub fn steps(&self) -> Vec<Box<dyn Step + '_>> {
        let mut messages = Vec::new();
        let skip = self.args.lookup(&SKIP_HOOKS, &mut messages);
        vec![
            Box::new(stages::Modules),
            Box::new(stages::HostId {
                spl_hostid: &self.options.spl_hostid,
            }),
            Box::new(stages::DeviceWait {
                wait_for: &self.options.wait_for_devices,
                retry_delay: &self.options.retry_delay,
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

    /// Runs the stages that `rerun` selects, and returns the default boot
    /// environment the import found.
    fn run_stages(&self, rerun: Rerun) -> Option<String> {
        let mut shell = self.shell();
        let mut context = StepContext::new(&self.system, &self.state, &mut shell);
        run_steps(&self.steps(), &mut context, rerun);
        context.bootfs.map(|dataset| dataset.to_string())
    }

    /// The countdown and the automatic boot of `bootfs`.
    fn countdown(&self, bootfs: Option<&str>) {
        let mut facts = Facts {
            bootfs: bootfs.map(str::to_owned),
            ..Facts::default()
        };
        let timeout = self.options.menu_timeout_seconds();
        let Some(bootfs) = bootfs.filter(|_| timeout.is_some_and(|timeout| timeout >= 0)) else {
            let _ = self
                .shell()
                .refuse(&support::unsupported(&self.options, &facts));
            return;
        };

        if self.count_down(bootfs, timeout.unwrap_or_default()) == Answer::Escape {
            facts.countdown_interrupted = true;
            let _ = self
                .shell()
                .refuse(&support::unsupported(&self.options, &facts));
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
    fn count_down(&self, bootfs: &str, seconds: i64) -> Answer {
        let width = self.options.menu_timeout.len();
        let name = colorize("cyan", bootfs);
        TimedPrompt::new()
            .delay(seconds)
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
    fn boot(&self, bootfs: &str) {
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
            let message = format!("unable to load key for {}", colorize("cyan", bootfs));
            self.shell().emergency_shell(&message);
            return;
        }
        if boot::find_kernels(system, &self.state, bootfs).is_ok() && !self.state.is_active() {
            let selected = boot::select_kernel(system, &self.state, bootfs)
                .unwrap_or_else(|| Kernel::parse(""));
            // 50-import-pools exported BOOTFS in the legacy program.
            let settings = BootSettings {
                set_hostid: self.options.set_hostid,
                hooks: self.hooks.clone(),
                env: vec![("BOOTFS".to_owned(), bootfs.to_owned())],
                save_logs: self.save_logs(),
            };
            let mut shell = self.shell();
            boot::kexec_kernel(system, &self.state, &settings, &selected, &mut shell);
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

            let settings = self.import_settings();
            let pool = settings
                .prefer_pool
                .as_ref()
                .filter(|_| settings.requirement == Requirement::Only);
            let reimport = Import::target(pool).policy(settings.policy);
            let _ = import::import_pool(&self.system, &self.state, reimport);
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

#[cfg(test)]
mod tests;
