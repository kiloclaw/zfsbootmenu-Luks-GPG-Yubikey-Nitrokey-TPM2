//! The application layer of ZFSBootMenu: what front ends can ask for, and
//! what they get back.
//!
//! Front ends (the menu, `zluks`, the start-up program) talk only to this
//! crate. They read the machine as a [`model::Snapshot`] ([`App::snapshot`]),
//! send an [`Action`] to [`App::perform`], and get [`Event`]s back, or an
//! [`AppError`] that keeps the cause of the failure. They never run programs
//! or call the domain crates themselves, which `cargo xtask check-deps`
//! enforces.
//!
//! The vocabulary is [`model`], the lists of
//! `docs/dev/menu-design/interfaces.md`, which the menu prototype tried on a
//! fake machine. R4 of REFACTOR-ROADMAP.md fills it from the real one; the
//! actions and events below are still those of the pilot (R0), which R4
//! replaces with the model's.
//!
//! ```
//! use zbm_app::{Action, App, Event, UnlockRequest};
//! use zbm_platform::Context;
//! use zbm_testkit::fakes::FakeSystem;
//!
//! let fake = FakeSystem::new();
//! fake.files.file("/keys/root.key", "secret");
//! let app = App::new(Context::new(fake.system()));
//!
//! let events = app.perform(Action::Unlock(UnlockRequest {
//!     device: "/dev/sda2".parse()?,
//!     name: "cryptroot".parse()?,
//!     key_file: "/keys/root.key".into(),
//!     options: Default::default(),
//! }))?;
//! assert!(matches!(&events[0], Event::Unlocked { method: "key file", .. }));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod boot;
pub mod model;
mod read;
mod unlock;
mod volumes;

pub use boot::kernel_version;
pub use model::Encryption;
pub use read::stage_name;
pub use unlock::UnlockRequest;
pub use volumes::{NO_CRYPTSETUP, device_not_found};
pub use zbm_boot::BootError;
pub use zbm_boot::boot::BootSettings;
pub use zbm_boot::recovery::Recovery;
pub use zbm_crypt::OpenOptions;
pub use zbm_luks::pilot::UnlockFailed;

use std::path::Path;

use zbm_config::options::ZbmOptions;
use zbm_luks::unlocker::Luks;
use zbm_platform::{Context, System};
use zbm_types::{DevicePath, MappingName};

/// What a front end asks the application to do.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Action {
    /// Open a LUKS volume.
    Unlock(UnlockRequest),
}

impl Action {
    /// What kind of action this is, for messages and the journal: `unlock`.
    pub fn kind(&self) -> &'static str {
        match self {
            Action::Unlock(_) => "unlock",
        }
    }
}

/// What happened while performing an [`Action`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// A LUKS volume is open.
    Unlocked {
        /// The volume.
        device: DevicePath,
        /// The name of its mapping.
        name: MappingName,
        /// The key method that opened it.
        method: &'static str,
    },
}

impl std::fmt::Display for Event {
    /// One line for people: `opened /dev/sda2 as /dev/mapper/cryptroot (key file)`.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Unlocked {
                device,
                name,
                method,
            } => write!(
                formatter,
                "opened {device} as {} ({method})",
                name.mapper_path()
            ),
        }
    }
}

/// Where events go: a front end that shows them, or a list.
pub trait Sink {
    /// Receives `event`.
    fn send(&mut self, event: model::Event);
}

impl Sink for Vec<model::Event> {
    fn send(&mut self, event: model::Event) {
        self.push(event);
    }
}

/// Why the machine could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SnapshotError {
    /// ZFS could not answer.
    #[error(transparent)]
    Zfs(#[from] zbm_zfs::ZfsError),
}

/// Why an [`Action`] failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AppError {
    /// No key method opened the volume; the error lists every attempt.
    #[error(transparent)]
    Unlock(#[from] UnlockFailed),
    /// The machine could not be read.
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
}

/// The application: the ports of a [`Context`] and the use cases on top.
#[derive(Clone, Copy)]
pub struct App<'a> {
    /// The ports the use cases reach the system through.
    context: Context<'a>,
    /// The options of this boot, when the front end read them.
    options: Option<&'a ZbmOptions>,
    /// The LUKS session, when the front end has one.
    luks: Option<Luks<'a>>,
    /// The state directory (`/zfsbootmenu`).
    base: Option<&'a Path>,
    /// The directory of the image's keyboard layouts.
    layouts: Option<&'a Path>,
    /// How to boot, when the front end can boot.
    boot: Option<&'a BootSettings>,
}

impl<'a> App<'a> {
    /// The application on `context`, knowing nothing of this boot yet.
    pub fn new(context: Context<'a>) -> App<'a> {
        App {
            context,
            options: None,
            luks: None,
            base: None,
            layouts: None,
            boot: None,
        }
    }

    /// With the options of this boot: the countdown and the console.
    #[must_use]
    pub fn with_options(self, options: &'a ZbmOptions) -> App<'a> {
        App {
            options: Some(options),
            ..self
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

    /// With the state directory `base`: the journal and the console's
    /// layout.
    #[must_use]
    pub fn with_base(self, base: &'a Path) -> App<'a> {
        App {
            base: Some(base),
            ..self
        }
    }

    /// With what booting needs besides the state directory: the hooks, the
    /// host ID and the saved logs.
    #[must_use]
    pub fn with_boot(self, settings: &'a BootSettings) -> App<'a> {
        App {
            boot: Some(settings),
            ..self
        }
    }

    /// With the directory of the image's keyboard layouts.
    #[must_use]
    pub fn with_layouts(self, directory: &'a Path) -> App<'a> {
        App {
            layouts: Some(directory),
            ..self
        }
    }

    /// Performs `action`.
    ///
    /// # Errors
    ///
    /// [`AppError`] with the cause when the action fails.
    pub fn perform(&self, action: Action) -> Result<Vec<Event>, AppError> {
        let _span = tracing::info_span!("action", kind = action.kind()).entered();
        match action {
            Action::Unlock(request) => unlock::perform(&self.context, request),
        }
    }

    /// Performs `action` of the model; what happened comes back as events,
    /// the last of them saying how the action ended. Actions the runtime
    /// cannot perform yet (most of them wait for R5) end as failed.
    pub fn act(&self, action: &model::Action) -> Vec<model::Event> {
        if let Some(luks) = self.luks
            && let Some(events) = volumes::perform(luks, action)
        {
            return events;
        }
        vec![model::Event::Finished(model::Outcome::Failed {
            what: "unable to do that here".to_owned(),
            why: vec![format!("{action:?} is not available in this program yet")],
            next: Vec::new(),
        })]
    }

    /// Performs `action`, one that takes the console
    /// ([`model::Action::needs_console`]): booting, which may ask for a
    /// passphrase, the recovery shell, rebooting and powering off. It
    /// returns only when the machine did not boot or reset, or the shell
    /// ended, with how the action ended as the last event. `console` is the
    /// front end's whole system, whose console the action takes; `recovery`
    /// opens the shell, and the recovery shells of a failed boot.
    pub fn act_on_console(
        &self,
        action: &model::Action,
        console: System<'_>,
        recovery: &mut dyn Recovery,
    ) -> Vec<model::Event> {
        let system = console.with_runner(self.context.runner());
        let pools =
            || read::pools(&zbm_zfs::ZfsCli::new(self.context.runner())).unwrap_or_default();
        match action {
            model::Action::Shell => {
                recovery.emergency_shell("");
                Vec::new()
            }
            model::Action::Reboot => boot::reset(&system, &pools(), "b"),
            model::Action::PowerOff => boot::reset(&system, &pools(), "o"),
            model::Action::FirmwareSetup => boot::firmware_setup(&system, &pools()),
            model::Action::Boot {
                environment,
                kernel,
                command_line,
                countdown,
            } => {
                let (Some(base), Some(settings)) = (self.base, self.boot) else {
                    return self.act(action);
                };
                let state = zbm_state::State::new(system.files, base);
                let boot = boot::Boot {
                    system: &system,
                    state: &state,
                    settings,
                };
                boot.boot(
                    environment,
                    kernel.as_deref(),
                    command_line.as_deref(),
                    *countdown,
                    recovery,
                )
            }
            other => self.act(other),
        }
    }

    /// Fills in the kernels of the environments of `snapshot` whose keys are
    /// loaded, by mounting each read-only, as the menu needs them; the
    /// snapshot alone mounts nothing. `console` is the front end's whole
    /// system.
    pub fn read_kernels(&self, snapshot: &mut model::Snapshot, console: System<'_>) {
        let (Some(base), Some(settings)) = (self.base, self.boot) else {
            return;
        };
        let system = console.with_runner(self.context.runner());
        let state = zbm_state::State::new(system.files, base);
        let boot = boot::Boot {
            system: &system,
            state: &state,
            settings,
        };
        for environment in &mut snapshot.environments {
            if environment.encryption != Encryption::Locked {
                environment.kernels = boot.kernels(environment.name.as_str());
            }
        }
    }

    /// Reads the machine: what every view of the menu reads. What the
    /// application was not given (options, a LUKS session, the state and
    /// layout directories) stays empty.
    ///
    /// # Errors
    ///
    /// [`AppError::Snapshot`] when `zfs` or `zpool` fails or prints something
    /// unexpected.
    pub fn snapshot(&self) -> Result<model::Snapshot, AppError> {
        let zfs = zbm_zfs::ZfsCli::new(self.context.runner());
        let files = self.context.files();
        let now = self
            .context
            .clock()
            .wall()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_secs());
        let keyboard = match (self.layouts, self.base) {
            (Some(layouts), Some(base)) => {
                let state = zbm_state::State::new(files, base);
                let console = self
                    .options
                    .map_or("", |options| options.control_term.as_str());
                read::keyboard(files, layouts, &state, console)
            }
            _ => model::Keyboard::default(),
        };
        let state = self.base.map(|base| zbm_state::State::new(files, base));
        let startup = state.as_ref().map(read::startup).unwrap_or_default();
        let bootfs = state
            .and_then(|state| state.bootfs())
            .and_then(|name| name.parse().ok());
        Ok(model::Snapshot {
            startup,
            pools: read::pools(&zfs).map_err(SnapshotError::from)?,
            environments: read::environments(&zfs).map_err(SnapshotError::from)?,
            volumes: self
                .luks
                .map(|luks| read::volumes(&zbm_luks::unlocker::Unlocker::new(luks)))
                .unwrap_or_default(),
            timeout: self.options.map(read::timeout).unwrap_or_default(),
            log: self
                .base
                .map(|base| read::log(files, base))
                .unwrap_or_default(),
            keyboard,
            now,
            bootfs,
            ..model::Snapshot::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::fakes::FakeSystem;

    #[test]
    fn actions_name_their_kind_and_the_snapshot_reads_the_machine() {
        let request = UnlockRequest {
            device: "/dev/sda2".parse().unwrap(),
            name: "cryptroot".parse().unwrap(),
            key_file: "/keys/root.key".into(),
            options: zbm_crypt::OpenOptions::default(),
        };
        assert_eq!(Action::Unlock(request).kind(), "unlock");

        let fake = FakeSystem::new();
        fake.runner.respond_program("zpool", 0, "");
        fake.runner.respond_program("zfs", 0, "");
        let app = App::new(Context::new(fake.system()));
        let snapshot = app.snapshot().unwrap();
        assert!(snapshot.environments.is_empty() && snapshot.volumes.is_empty());
    }

    #[test]
    fn the_snapshot_holds_what_the_app_was_given() {
        let fake = FakeSystem::new();
        fake.runner.respond_program("zpool", 0, "");
        fake.runner.respond_program("zfs", 0, "");
        fake.files.file("/layouts/layouts", "us\nde\n").file(
            "/zfsbootmenu/journal.jsonl",
            "{\"level\":\"WARN\",\"message\":\"slow\"}\n",
        );
        let (mut options, _) =
            ZbmOptions::parse(&zbm_config::args::ZbmArgs::from_kcl("zbm.timeout=7"));
        options.control_term = "ttyS0".into();
        let settings = zbm_luks::unlocker::Settings::new("/zfsbootmenu");
        let prompter = crate::read::tests::NoPrompts;
        let luks = Luks {
            system: fake.system(),
            settings: &settings,
            prompter: &prompter,
        };

        let snapshot = App::new(Context::new(fake.system()))
            .with_options(&options)
            .with_luks(luks)
            .with_base(Path::new("/zfsbootmenu"))
            .with_layouts(Path::new("/layouts"))
            .snapshot()
            .unwrap();

        assert_eq!(snapshot.timeout, model::Timeout::Seconds(7));
        assert_eq!(snapshot.keyboard.layouts, ["us", "de"]);
        assert!(!snapshot.keyboard.local);
        assert_eq!(snapshot.log.count(model::Level::Warning), 1);
        assert!(snapshot.volumes.is_empty() && snapshot.now > 0);
    }
}
