//! The application layer of ZFSBootMenu: what front ends can ask for, and
//! what they get back.
//!
//! Front ends (the menu, `zluks`, the start-up program) talk only to this
//! crate. They read the machine as a [`model::Snapshot`]
//! ([`Application::snapshot`]), send a [`model::Action`] to
//! [`Application::apply`], and get [`model::Event`]s back, the last of them
//! saying how the action ended. The application is [`App`]: the ports of a
//! [`zbm_platform::Context`] and what it knows of this boot, its
//! [`Runtime`], which it always has (ROADMAP.md, DM8). The actions of the
//! LUKS volumes alone are [`Volumes`]'. Front ends never run programs or
//! call the domain crates themselves, which `cargo xtask check-deps`
//! enforces.
//!
//! The vocabulary is [`model`], the lists of
//! `docs/dev/menu-design/interfaces.md`, which the menu prototype tried on a
//! fake machine.

mod application;
mod boot;
mod changes;
pub mod console;
pub mod model;
mod plan;
mod read;
pub mod service;
pub mod session;
mod volumes;
pub mod wire;

pub use application::{App, Application, LAYOUTS, Runtime, Through, Volumes};
pub use boot::kernel_version;
pub use model::Encryption;
pub use read::stage_name;
pub use volumes::{NO_CRYPTSETUP, device_not_found};
pub use zbm_boot::BootError;
pub use zbm_boot::boot::BootSettings;
pub use zbm_boot::recovery::Recovery;
pub use zbm_crypt::OpenOptions;

/// Why the machine could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SnapshotError {
    /// ZFS could not answer.
    #[error(transparent)]
    Zfs(#[from] zbm_zfs::ZfsError),
}

/// Why the application could not do what was asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AppError {
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

#[cfg(test)]
mod tests {
    use zbm_types::code::Coded;

    use super::*;

    #[test]
    fn an_error_of_the_application_has_the_code_of_its_cause() {
        let zfs = || zbm_zfs::ZfsError::Failed {
            command: "zpool list".to_owned(),
            status: 1,
            message: "no pools".to_owned(),
        };

        let error = AppError::from(SnapshotError::from(zfs()));

        assert_eq!(error.code(), zfs().code());
    }
}
