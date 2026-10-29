//! The application layer of ZFSBootMenu: what front ends can ask for, and
//! what they get back.
//!
//! Front ends (the menu, `zluks`, the start-up program) talk only to this
//! crate. They read the machine as a [`model::Snapshot`]
//! ([`Application::snapshot`]), send a [`model::Action`] to
//! [`Application::apply`], and get [`model::Event`]s back, the last of them
//! saying how the action ended. The application is [`App`]: the ports of a
//! [`Context`] and what it knows of this boot, its [`Runtime`], which it
//! always has (ROADMAP.md, DM8). The actions of the LUKS volumes alone are
//! [`Volumes`]'. Front ends never run programs or call the domain crates
//! themselves, which `cargo xtask check-deps` enforces.
//!
//! The vocabulary is [`model`], the lists of
//! `docs/dev/menu-design/interfaces.md`, which the menu prototype tried on a
//! fake machine. The [`Action`] and [`Event`] below are still those of the
//! pilot (R0), `zluks --pilot`, which [`pilot`] performs.
//!
//! ```
//! use zbm_app::{Action, Event, UnlockRequest, pilot};
//! use zbm_platform::Context;
//! use zbm_testkit::fakes::FakeSystem;
//!
//! let fake = FakeSystem::new();
//! fake.files.file("/keys/root.key", "secret");
//!
//! let request = UnlockRequest {
//!     device: "/dev/sda2".parse()?,
//!     name: "cryptroot".parse()?,
//!     key_file: "/keys/root.key".into(),
//!     options: Default::default(),
//! };
//! let events = pilot(&Context::new(fake.system()), Action::Unlock(request))?;
//! assert!(matches!(&events[0], Event::Unlocked { method: "key file", .. }));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod application;
mod boot;
mod changes;
pub mod console;
pub mod model;
mod plan;
mod read;
pub mod service;
pub mod session;
mod unlock;
mod volumes;
pub mod wire;

pub use application::{App, Application, LAYOUTS, Runtime, Through, Volumes};
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

use zbm_platform::Context;
use zbm_types::{DevicePath, MappingName};

/// What `zluks --pilot` asks the application to do.
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

impl zbm_types::code::Coded for SnapshotError {
    fn code(&self) -> zbm_types::code::ErrorCode {
        match self {
            SnapshotError::Zfs(error) => zbm_types::code::Coded::code(error),
        }
    }
}

impl zbm_types::code::Coded for AppError {
    fn code(&self) -> zbm_types::code::ErrorCode {
        match self {
            AppError::Unlock(error) => zbm_types::code::Coded::code(error),
            AppError::Snapshot(error) => zbm_types::code::Coded::code(error),
        }
    }
}

/// The codes of the adapters' errors, which front ends reach through this
/// crate, for `zbm explain`.
pub mod codes {
    pub use zbm_crypt::CRYPT_CODES;
    pub use zbm_zfs::ZFS_CODES;
}

/// Performs `action` of the pilot (R0) on the ports of `context`.
///
/// # Errors
///
/// [`AppError`] with the cause when the action fails.
pub fn pilot(context: &Context<'_>, action: Action) -> Result<Vec<Event>, AppError> {
    let _span = tracing::info_span!("action", kind = action.kind()).entered();
    match action {
        Action::Unlock(request) => unlock::perform(context, request),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_name_their_kind() {
        let request = UnlockRequest {
            device: "/dev/sda2".parse().unwrap(),
            name: "cryptroot".parse().unwrap(),
            key_file: "/keys/root.key".into(),
            options: zbm_crypt::OpenOptions::default(),
        };
        assert_eq!(Action::Unlock(request).kind(), "unlock");
    }
}
