//! The application layer of ZFSBootMenu: what front ends can ask for, and
//! what they get back.
//!
//! Front ends (the menu, `zluks`, the start-up program) talk only to this
//! crate. They read the machine as a [`Snapshot`], send an [`Action`] to
//! [`App::perform`], and get [`Event`]s back, or an [`AppError`] that keeps
//! the cause of the failure. They never run programs or call the domain
//! crates themselves, which `cargo xtask check-deps` enforces.
//!
//! This is the pilot of REFACTOR-ROADMAP.md (R0); R4 adds the remaining use
//! cases.
//!
//! ```
//! use zbm_app::{Action, App, Event, UnlockRequest};
//! use zbm_platform::Context;
//! use zbm_testutil::fakes::FakeSystem;
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

mod snapshot;
mod unlock;

pub use snapshot::{BootEnvironment, Encryption, Snapshot, SnapshotError};
pub use unlock::UnlockRequest;
pub use zbm_crypt::OpenOptions;
pub use zbm_luks::unlocker::UnlockFailed;

use zbm_platform::Context;
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
}

impl<'a> App<'a> {
    /// The application on `context`.
    pub fn new(context: Context<'a>) -> App<'a> {
        App { context }
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

    /// Reads the machine: its boot environments.
    ///
    /// # Errors
    ///
    /// [`AppError::Snapshot`] when `zfs` or `zpool` fails or prints something
    /// unexpected.
    pub fn snapshot(&self) -> Result<Snapshot, AppError> {
        Ok(snapshot::read(self.context.runner())?)
    }
}
