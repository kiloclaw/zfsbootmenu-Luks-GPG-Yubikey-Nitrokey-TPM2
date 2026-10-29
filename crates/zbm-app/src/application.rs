//! The application on the real machine: [`App`], what it knows of this
//! boot ([`Runtime`]), and how front ends reach it ([`Application`]): they
//! read the machine and apply actions to it, each through the front end's
//! system, quietly or on its console ([`Through`]).

use std::path::{Path, PathBuf};

use zbm_boot::boot::BootSettings;
use zbm_boot::recovery::Recovery;
use zbm_config::args::ZbmArgs;
use zbm_config::options::ZbmOptions;
use zbm_luks::unlocker::Luks;
use zbm_platform::{Context, System};

use crate::model::{Action, Encryption, Event, Keyboard, Outcome, Snapshot};
use crate::{AppError, SnapshotError, boot, changes, read, volumes};

/// Where the image keeps its keyboard layouts.
pub const LAYOUTS: &str = "/usr/share/zfsbootmenu/layouts";

/// What the application knows of this boot, which every use case may need:
/// the options, the state directory, the keyboard layouts and how to boot.
#[derive(Clone, Debug)]
pub struct Runtime {
    /// The options of this boot: the countdown and the console.
    pub options: ZbmOptions,
    /// The state directory (`/zfsbootmenu`): the journal, the stages, the
    /// console's layout.
    pub base: PathBuf,
    /// The directory of the image's keyboard layouts.
    pub layouts: PathBuf,
    /// How to boot besides the state directory: the hooks, the host ID and
    /// the saved logs.
    pub boot: BootSettings,
}

impl Runtime {
    /// The runtime of a boot with `options` and the state directory
    /// `base`, the image's layouts, and the default way to boot.
    pub fn new(options: ZbmOptions, base: &Path) -> Runtime {
        Runtime {
            options,
            base: base.to_owned(),
            layouts: PathBuf::from(LAYOUTS),
            boot: BootSettings::default(),
        }
    }

    /// The runtime, whose state directory is `base`.
    #[must_use]
    pub fn with_base(mut self, base: &Path) -> Runtime {
        base.clone_into(&mut self.base);
        self
    }

    /// The runtime of a boot with `options`.
    #[must_use]
    pub fn with_options(mut self, options: ZbmOptions) -> Runtime {
        self.options = options;
        self
    }

    /// The runtime, whose layouts are in `directory`.
    #[must_use]
    pub fn with_layouts(mut self, directory: &Path) -> Runtime {
        directory.clone_into(&mut self.layouts);
        self
    }

    /// The runtime, which boots with `settings`.
    #[must_use]
    pub fn with_boot(mut self, settings: BootSettings) -> Runtime {
        self.boot = settings;
        self
    }
}

impl Default for Runtime {
    /// The options of an empty command line, in `/zfsbootmenu`.
    fn default() -> Runtime {
        let (options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(""));
        Runtime::new(options, Path::new(zbm_state::BASE))
    }
}

/// Through what an action reaches the machine.
#[derive(Clone, Copy)]
pub enum Through<'s> {
    /// The front end's whole system, whose console is left alone: a change
    /// that would have to ask for something fails, and is applied on the
    /// console instead when [`Action::needs_console_on`] says so.
    Quietly(System<'s>),
    /// The front end's whole system, whose console the action takes, and
    /// what opens the recovery shell, and the recovery shells of a failed
    /// boot.
    Console(System<'s>, &'s dyn Recovery),
}

/// What front ends reach the machine through: [`App`], or a fake of it.
pub trait Application {
    /// Reads the machine: what every view of the menu reads.
    ///
    /// # Errors
    ///
    /// [`AppError::Snapshot`] when `zfs` or `zpool` fails or prints
    /// something unexpected.
    fn snapshot(&self) -> Result<Snapshot, AppError>;

    /// Fills in the kernels of the environments of `snapshot` whose keys
    /// are loaded, which mounts each read-only on `console`, the front
    /// end's whole system; the snapshot alone mounts nothing.
    fn read_kernels(&self, snapshot: &mut Snapshot, console: System<'_>);

    /// Applies `action` through `through`; what happened comes back as
    /// events, the last of them saying how the action ended. One that takes
    /// the console returns only when the machine did not boot or reset, or
    /// the shell ended.
    fn apply(&self, action: &Action, through: Through<'_>) -> Vec<Event>;
}

/// The actions of the LUKS volumes alone, which need no more than a LUKS
/// session: `zbm luks` and start-up's unlocking.
#[derive(Clone, Copy)]
pub struct Volumes<'a>(Luks<'a>);

impl<'a> Volumes<'a> {
    /// The volumes of the LUKS session `luks`.
    pub fn new(luks: Luks<'a>) -> Volumes<'a> {
        Volumes(luks)
    }

    /// Applies `action`, which ends as failed unless it is one of the
    /// volumes'.
    pub fn apply(&self, action: &Action) -> Vec<Event> {
        volumes::perform(self.0, action).unwrap_or_else(|| unavailable(action))
    }
}

/// The end of an action the runtime cannot perform yet.
pub(crate) fn unavailable(action: &Action) -> Vec<Event> {
    vec![Event::Finished(Outcome::Failed {
        what: "unable to do that here".to_owned(),
        why: vec![format!("{action:?} is not available in this program yet")],
        next: Vec::new(),
    })]
}

/// The application: the ports of a [`Context`], what it knows of this boot
/// and, when the front end has one, a LUKS session.
#[derive(Clone, Copy)]
pub struct App<'a> {
    /// The ports the use cases reach the system through.
    pub(crate) context: Context<'a>,
    /// What it knows of this boot.
    pub(crate) runtime: &'a Runtime,
    /// The LUKS session, when the front end has one.
    luks: Option<Luks<'a>>,
}

impl<'a> App<'a> {
    /// The application on `context`, for the boot `runtime` describes.
    pub fn new(context: Context<'a>, runtime: &'a Runtime) -> App<'a> {
        App {
            context,
            runtime,
            luks: None,
        }
    }

    /// With a LUKS session: the volumes.
    #[must_use]
    pub fn with_luks(self, luks: Luks<'a>) -> App<'a> {
        App {
            luks: Some(luks),
            ..self
        }
    }

    /// The application with the programs of its context run by `runner`,
    /// such as one that cancels them. A LUKS session keeps its own.
    #[must_use]
    pub fn with_runner<'b>(&self, runner: &'b dyn zbm_platform::Runner) -> App<'b>
    where
        'a: 'b,
    {
        App {
            context: self.context.with_runner(runner),
            ..*self
        }
    }

    /// The runner of the application's context.
    pub fn runner(&self) -> &dyn zbm_platform::Runner {
        self.context.runner()
    }

    /// Performs `action` with nothing but the application's own ports: the
    /// actions of the LUKS volumes; the others end as failed.
    pub(crate) fn alone(&self, action: &Action) -> Vec<Event> {
        match self.luks {
            Some(luks) => Volumes::new(luks).apply(action),
            None => unavailable(action),
        }
    }

    /// Applies `action`, one that may take the console
    /// ([`Action::needs_console`]): booting, which may ask for a
    /// passphrase, the recovery shell, rebooting and powering off.
    fn on_console(
        &self,
        action: &Action,
        console: System<'_>,
        recovery: &dyn Recovery,
    ) -> Vec<Event> {
        let system = console.with_runner(self.context.runner());
        let pools =
            || read::pools(&zbm_zfs::ZfsCli::new(self.context.runner())).unwrap_or_default();
        match action {
            Action::Shell => {
                recovery.emergency_shell("");
                Vec::new()
            }
            Action::Reboot => boot::reset(&system, &pools(), "b"),
            Action::PowerOff => boot::reset(&system, &pools(), "o"),
            Action::FirmwareSetup => boot::firmware_setup(&system, &pools()),
            change if change.is_change() => self.change(change, &system, true),
            Action::Boot {
                environment,
                kernel,
                command_line,
                countdown,
            } => {
                let state = zbm_state::State::new(system.files, &self.runtime.base);
                let boot = boot::Boot {
                    system: &system,
                    state: &state,
                    settings: &self.runtime.boot,
                };
                boot.boot(
                    environment,
                    kernel.as_ref().map(|kernel| kernel.get().as_str()),
                    command_line.as_ref().map(|line| line.get().as_str()),
                    *countdown,
                    recovery,
                )
            }
            other => self.alone(other),
        }
    }

    /// Applies `action` without taking the console: the volumes', the
    /// changes of package D ([`Action::is_change`]) whose pool is writable
    /// and whose key is loaded, and the dry runs of [`Action::Plan`]
    /// ([`App::plan`]).
    fn quietly(&self, action: &Action, console: System<'_>) -> Vec<Event> {
        if let Action::Plan(planned) = action {
            return self.planned(planned, console);
        }
        if !action.is_change() {
            return self.alone(action);
        }
        let system = console.with_runner(self.context.runner());
        self.change(action, &system, false)
    }

    /// Performs the change `action` on `system`, asking on its console
    /// when `asks`.
    fn change(&self, action: &Action, system: &System<'_>, asks: bool) -> Vec<Event> {
        let state = zbm_state::State::new(system.files, &self.runtime.base);
        let zfs = zbm_zfs::ZfsCli::new(system.runner);
        let mut needs = changes::OnMachine {
            system,
            state: &state,
            settings: &self.runtime.boot,
            asks,
        };
        let mut changes = changes::Changes {
            zfs: &zfs,
            needs: &mut needs,
        };
        changes
            .perform(action)
            .unwrap_or_else(|| self.alone(action))
    }

    /// The keyboard layouts secrets can be typed with, and whether keys
    /// come from the machine's own keyboard; it reads no pool, so start-up
    /// can ask it before any is imported.
    pub fn keyboard(&self) -> Keyboard {
        let files = self.context.files();
        let runtime = self.runtime;
        let state = zbm_state::State::new(files, &runtime.base);
        read::keyboard(
            files,
            &runtime.layouts,
            &state,
            &runtime.options.control_term,
        )
    }
}

impl Application for App<'_> {
    fn snapshot(&self) -> Result<Snapshot, AppError> {
        let zfs = zbm_zfs::ZfsCli::new(self.context.runner());
        let files = self.context.files();
        let now = self
            .context
            .clock()
            .wall()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_secs());
        let state = zbm_state::State::new(files, &self.runtime.base);
        let bootfs = state.bootfs().and_then(|name| name.parse().ok());
        Ok(Snapshot {
            startup: read::startup(&state),
            pools: read::pools(&zfs).map_err(SnapshotError::from)?,
            environments: read::environments(&zfs).map_err(SnapshotError::from)?,
            volumes: self
                .luks
                .map(|luks| read::volumes(&zbm_luks::unlocker::Unlocker::new(luks)))
                .unwrap_or_default(),
            timeout: read::timeout(&self.runtime.options),
            log: read::log(files, &self.runtime.base),
            keyboard: self.keyboard(),
            now,
            bootfs,
            ..Snapshot::default()
        })
    }

    fn read_kernels(&self, snapshot: &mut Snapshot, console: System<'_>) {
        let system = console.with_runner(self.context.runner());
        let state = zbm_state::State::new(system.files, &self.runtime.base);
        let boot = boot::Boot {
            system: &system,
            state: &state,
            settings: &self.runtime.boot,
        };
        for environment in &mut snapshot.environments {
            if environment.encryption != Encryption::Locked {
                environment.kernels = boot.kernels(&environment.name);
            }
        }
    }

    fn apply(&self, action: &Action, through: Through<'_>) -> Vec<Event> {
        match through {
            Through::Quietly(console) => self.quietly(action, console),
            Through::Console(console, recovery) => self.on_console(action, console, recovery),
        }
    }
}

#[cfg(test)]
mod tests;
