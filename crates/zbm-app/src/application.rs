//! The application on the real machine: [`App`], what it knows of this
//! boot ([`Runtime`]), and how front ends reach it ([`Application`]): they
//! read the machine and apply actions to it, each through the front end's
//! system, quietly or on its console ([`Through`]).

use std::path::{Path, PathBuf};

use zbm_boot::BootSettings;
use zbm_boot::Recovery;
use zbm_config::args::ZbmArgs;
use zbm_config::options::ZbmOptions;
use zbm_luks::unlocker::Luks;
use zbm_platform::event::CancelToken;
use zbm_platform::{Context, System};
use zbm_pool::ImportSettings;

use zbm_tools::programs::ZFS_CHROOT;
use zbm_types::{DatasetName, FilesystemName};

use crate::auth::{self, Authority};
use crate::keyholder::Keyholders;
use crate::model::{Action, Encryption, Event, Keyboard, Offer, Outcome, Snapshot};
use crate::volumes::Volumes;
use crate::{AppError, SnapshotError, boot, read};

/// Where the image keeps its keyboard layouts.
pub const LAYOUTS: &str = "/usr/share/zfsbootmenu/layouts";

/// What the application knows of this boot, which every use case may need:
/// the options, the state directory, the keyboard layouts, how to boot and
/// how to import pools.
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
    /// How pools are imported, as start-up imports them: what an import
    /// of a pool that is not imported reads.
    pub import: ImportSettings,
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
            import: ImportSettings::default(),
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

    /// The runtime, which imports pools with `settings`.
    #[must_use]
    pub fn with_import(mut self, settings: ImportSettings) -> Runtime {
        self.import = settings;
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
    /// The lock of the menu, and the proof of who may do what it keeps
    /// (ROADMAP.md SE9).
    authority: &'a Authority,
    /// What checks a PIN; without them, what the lock keeps is refused.
    keyholders: Option<&'a dyn Keyholders>,
    /// What cancels the action performed, which ends its keyholder.
    until: Option<&'a CancelToken>,
    /// What the front end keeps between reads: the pools the last scan
    /// found.
    seen: Option<&'a Seen>,
}

impl<'a> App<'a> {
    /// The application on `context`, for the boot `runtime` describes.
    pub fn new(context: Context<'a>, runtime: &'a Runtime) -> App<'a> {
        App {
            context,
            runtime,
            luks: None,
            authority: auth::process(),
            keyholders: None,
            until: None,
            seen: None,
        }
    }

    /// With `seen` keeping what the scan of the pools finds, which every
    /// snapshot lists then.
    #[must_use]
    pub fn with_seen(self, seen: &'a Seen) -> App<'a> {
        App {
            seen: Some(seen),
            ..self
        }
    }

    /// With `keyholders` to check the PIN of a locked menu.
    #[must_use]
    pub fn with_keyholders(self, keyholders: &'a dyn Keyholders) -> App<'a> {
        App {
            keyholders: Some(keyholders),
            ..self
        }
    }

    /// Behind the lock of `authority`, for tests.
    #[must_use]
    pub fn with_authority(self, authority: &'a Authority) -> App<'a> {
        App { authority, ..self }
    }

    /// Whether what the lock keeps is locked now, on the machine of `files`.
    fn is_menu_locked(&self, files: &dyn zbm_platform::Files) -> bool {
        let state = zbm_state::State::new(files, &self.runtime.base);
        self.authority
            .is_locked(state.has_attended_unlock(), self.context.clock().now())
    }

    /// Whether `action` may run: the lock does not keep it, or someone
    /// proved themselves, as a person who opened a volume did, or by the
    /// PIN typed on `system`'s console now, which a keyholder checks.
    fn is_allowed(&self, action: &Action, system: &System<'_>) -> bool {
        let state = zbm_state::State::new(system.files, &self.runtime.base);
        let bootfs: Option<DatasetName> = state.bootfs().and_then(|name| name.parse().ok());
        if !action.is_kept_by_lock(bootfs.as_ref()) {
            return true;
        }
        let now = system.clock.now();
        if self
            .authority
            .authorized(state.has_attended_unlock(), now)
            .is_some()
        {
            return true;
        }
        let Some(keyholders) = self.keyholders else {
            return false;
        };
        let console = system.console;
        let _ = console.write(auth::LOCKED_PROMPT.as_bytes());
        let pin = match console.read_secret() {
            Ok(Some(pin)) if !pin.is_empty() => pin,
            _ => return false,
        };
        let _ = console.write(b"\n");
        auth::check_pin(self.authority, keyholders, &pin, system.clock.now()).unwrap_or_else(
            |error| {
                tracing::error!(%error, "the PIN could not be checked");
                false
            },
        )
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

    /// The runner of the LUKS session's context, with a session.
    pub(crate) fn luks_runner(&self) -> Option<&dyn zbm_platform::Runner> {
        self.luks.as_ref().map(|luks| luks.context.runner())
    }

    /// The application whose action `until` cancels: `runner` runs the
    /// programs of its context, `luks_runner` those of its LUKS session,
    /// and a keyholder that opens a volume ends at once.
    pub(crate) fn cancelled_by<'b>(
        &self,
        until: &'b CancelToken,
        runner: &'b dyn zbm_platform::Runner,
        luks_runner: Option<&'b dyn zbm_platform::Runner>,
    ) -> App<'b>
    where
        'a: 'b,
    {
        let luks = self.luks.map(|luks| match luks_runner {
            Some(runner) => Luks {
                context: luks.context.with_runner(runner),
                ..luks
            },
            None => luks,
        });
        App {
            luks,
            until: Some(until),
            ..self.with_runner(runner)
        }
    }

    /// Performs `action` with nothing but the application's own ports: the
    /// actions of the LUKS volumes, which a keyholder performs when the
    /// application has them; the others end as failed.
    pub(crate) fn alone(&self, action: &Action) -> Vec<Event> {
        let Some(luks) = self.luks else {
            return unavailable(action);
        };
        let volumes = Volumes::new(luks);
        match self.keyholders {
            Some(keyholders) => volumes.through(keyholders).until(self.until),
            None => volumes,
        }
        .apply(action)
    }

    /// Applies `action`, one that may take the console
    /// ([`Action::needs_console`]): booting, which may ask for a
    /// passphrase, a snapshot's too, the recovery shell and the chroot,
    /// rebooting and powering off, an import, and a change that may ask.
    fn on_console(
        &self,
        action: &Action,
        console: System<'_>,
        recovery: &dyn Recovery,
    ) -> Vec<Event> {
        let system = console.with_runner(self.context.runner());
        if !self.is_allowed(action, &system) {
            return auth::refused();
        }
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
            Action::BootSnapshot(_, target) => self.boot_clone(action, target, &system, recovery),
            Action::Chroot(filesystem) => self.chroot(action, filesystem, &system),
            Action::ScanPools => self.scan(),
            Action::LoadDeviceLog(device) => self.device_log(device),
            Action::LoadSnapshots(environment) => self.load_snapshots(environment),
            Action::LoadHistory => self.history(console.runner),
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

    /// Clones the snapshot of `action`, a [`Action::BootSnapshot`], into
    /// `target` as a change on the console of `system`, then, once the
    /// clone is done, boots `target` as a boot chosen in the menu, with its
    /// own kernel and command line. When the boot fails, the clone stays,
    /// and the failure says so.
    fn boot_clone(
        &self,
        action: &Action,
        target: &DatasetName,
        system: &System<'_>,
        recovery: &dyn Recovery,
    ) -> Vec<Event> {
        let mut events = self.change(action, system, true);
        let Some(Event::Finished(Outcome::Done(cloned))) = events.last() else {
            return events;
        };
        let cloned = cloned.clone();
        events.pop();
        let state = zbm_state::State::new(system.files, &self.runtime.base);
        let boot = boot::Boot {
            system,
            state: &state,
            settings: &self.runtime.boot,
        };
        let mut booted = boot.boot(target, None, None, false, recovery);
        if let Some(Event::Finished(Outcome::Failed { next, .. })) = booted.last_mut() {
            next.push(format!("{cloned}: it stays, to boot or destroy"));
        }
        events.extend(booted);
        events
    }

    /// Opens a shell in a chroot of `filesystem` on the console of `system`
    /// with `zfs-chroot`, through the shell's gate (ADR 0010), unless the
    /// image has no `zfs-chroot`. It is done once a shell ran: `zfs-chroot`
    /// exits 0 then, whatever the shell's status, and otherwise, when it
    /// could not mount the filesystem (one whose key is not loaded) or run
    /// a shell in it, says why on the console and fails.
    fn chroot(
        &self,
        action: &Action,
        filesystem: &FilesystemName,
        system: &System<'_>,
    ) -> Vec<Event> {
        if !system.files.exists(Path::new(ZFS_CHROOT)) {
            return unavailable(action);
        }
        let state = zbm_state::State::new(system.files, &self.runtime.base);
        let now = system.clock.now();
        let outcome = match auth::chroot(self.authority, &state, system.runner, now, filesystem) {
            None => return auth::refused(),
            Some(Ok(ran)) if ran.status.is_success() => {
                Outcome::Done(format!("the shell in {filesystem} ended"))
            }
            Some(Ok(ran)) => Outcome::Failed {
                what: format!("unable to open a shell in {filesystem}"),
                why: vec![format!(
                    "zfs-chroot ended with status {}, after saying why on the console",
                    ran.status.shell_code()
                )],
                next: vec![format!(
                    "if {filesystem} is locked, load its key, then try again"
                )],
            },
            Some(Err(error)) => Outcome::Failed {
                what: format!("unable to open a shell in {filesystem}"),
                why: vec![error.to_string()],
                next: Vec::new(),
            },
        };
        vec![Event::Finished(outcome)]
    }

    /// The kinds of action the application performs on the machine of
    /// `files`: the boot, a snapshot's too, the changes of package D
    /// ([`Action::is_change`]), the default kernel, restoring files, the
    /// import and the export among them, the recovery shell, the chroot
    /// when the image has `zfs-chroot`, the power actions, and opening a
    /// volume with a LUKS session in an image with `cryptsetup`. The others
    /// have no use case yet, and end as [`unavailable`].
    fn offers(&self, files: &dyn zbm_platform::Files) -> std::collections::BTreeSet<Offer> {
        let chroot = files.exists(Path::new(ZFS_CHROOT)).then_some(Offer::Chroot);
        let open = self
            .luks
            .filter(|luks| luks.context.command_exists("cryptsetup"))
            .map(|_| Offer::OpenVolume);
        [
            Offer::Boot,
            Offer::BootSnapshot,
            Offer::SetDefaultEnvironment,
            Offer::DefaultKernel,
            Offer::CreateSnapshot,
            Offer::Rollback,
            Offer::Clone,
            Offer::Duplicate,
            Offer::Diff,
            Offer::RestoreFiles,
            Offer::Import,
            Offer::Export,
            Offer::Shell,
            Offer::FirmwareSetup,
            Offer::Power,
        ]
        .into_iter()
        .chain(chroot)
        .chain(open)
        .collect()
    }

    /// The LUKS volumes of the session, when the application has one.
    pub(crate) fn volumes(&self) -> Vec<crate::model::Volume> {
        self.luks
            .map(|luks| read::volumes(&zbm_luks::unlocker::Unlocker::new(luks)))
            .unwrap_or_default()
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
        let pools = self.pools(&zfs).map_err(SnapshotError::from)?;
        // The default stands on an imported pool: none once it is exported.
        let bootfs = state
            .bootfs()
            .and_then(|name| name.parse::<DatasetName>().ok())
            .filter(|bootfs| {
                let pool = bootfs.pool();
                pools
                    .iter()
                    .any(|known| known.imported && known.name == pool)
            });
        let (environments, loaded) = self.environments(&zfs).map_err(SnapshotError::from)?;
        Ok(Snapshot {
            startup: read::startup(&state),
            pools,
            loaded,
            environments,
            volumes: self.volumes(),
            timeout: read::timeout(&self.runtime.options),
            log: read::log(files, &self.runtime.base),
            keyboard: self.keyboard(),
            now,
            bootfs,
            menu_locked: self.is_menu_locked(files),
            offers: self.offers(files),
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

    /// Applies `action`, then, when it may have imported, exported or
    /// opened what holds a pool, scans the pools again for the snapshots
    /// read after it.
    fn apply(&self, action: &Action, through: Through<'_>) -> Vec<Event> {
        let events = match through {
            Through::Quietly(console) => self.quietly(action, console),
            Through::Console(console, recovery) => self.on_console(action, console, recovery),
        };
        if reads::may_move_pools(action) {
            self.rescan();
        }
        events
    }
}

mod reads;
mod seen;

pub use seen::Seen;

#[cfg(test)]
mod tests;
