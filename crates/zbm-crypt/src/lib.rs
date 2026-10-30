//! Typed wrappers around the encryption programs ZFSBootMenu runs.
//!
//! This is an adapter in the sense of `docs/dev/architecture.md`: it turns a
//! request ("open this LUKS volume with this key") into a program run through
//! the `Runner` port, and the program's answer into Rust values. No other
//! crate builds `cryptsetup` command lines.
//!
//! Unlocking reaches LUKS volumes through the [`LuksVolumes`] port, which
//! [`Cryptsetup`] implements. A mapping is a [`Mapping<Closed>`](Mapping)
//! until [`LuksVolumes::open`] opens it and returns a
//! [`Mapping<Open>`](Mapping); only an open mapping can be closed, and
//! closing hands back a closed one. The compiler therefore rejects closing a
//! volume that was never opened. A failure hands the mapping back unchanged,
//! in [`Failed`], with a [`CryptError`] that says why.
//!
//! Key material reaches `cryptsetup` only on standard input
//! (`--key-file=-`), never as an argument (see "Secrets" in
//! `crates/DESIGN.md`); a key file on a file system is read where it is.
//!
//! ```
//! use zbm_crypt::{Closed, Cryptsetup, Key, LuksVolumes, Mapping};
//! use zbm_platform::Secret;
//! use zbm_testkit::FakeRunner;
//!
//! let runner = FakeRunner::new();
//! let cryptsetup = Cryptsetup::new(&runner);
//! let (device, name) = ("/dev/sda2".parse()?, "cryptroot".parse()?);
//! let volume = Mapping::<Closed>::new(device, name);
//!
//! let key = Secret::copy_from(b"key");
//! let open = cryptsetup
//!     .open(volume, Key::Secret(&key), &[])
//!     .map_err(|failed| failed.error)?;
//! assert_eq!(open.path(), "/dev/mapper/cryptroot");
//! assert_eq!(
//!     runner.lines(),
//!     ["cryptsetup open --type luks --key-file=- /dev/sda2 cryptroot"]
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]

pub mod clevis;
mod cryptsetup;
pub mod fido2;
pub mod gpg;
mod mapping;
pub mod measure;
mod port;
pub mod token;
pub mod tpm2;
pub mod yubikey;

pub use cryptsetup::{Cryptsetup, OpenOptions};
pub use mapping::{Closed, Failed, Mapping, Open};
pub use port::{Key, LuksVolumes};
pub use token::{Token, TokenError};

use zbm_types::{DevicePath, MappingName};

/// Why `cryptsetup` did not do what was asked.
///
/// The variants follow the exit codes of `cryptsetup(8)`, so a caller can
/// tell a wrong key, which another key method may fix, from a missing device
/// or a busy mapping, which it cannot.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CryptError {
    /// `cryptsetup` could not be started.
    #[error("cryptsetup could not be started")]
    Start(#[source] std::io::Error),
    /// `cryptsetup` is not installed (exit status 127).
    #[error("cryptsetup is not installed")]
    NotInstalled,
    /// No key slot accepts the key (exit status 2).
    #[error("no key slot of {device} accepts the key")]
    WrongKey {
        /// The volume.
        device: DevicePath,
    },
    /// The device does not exist or is not a LUKS volume (exit status 4).
    #[error("{device} does not exist or is not a LUKS volume")]
    NoDevice {
        /// The volume.
        device: DevicePath,
    },
    /// The mapping exists already, or is in use (exit status 5).
    #[error("the mapping {name} exists already or is busy")]
    Busy {
        /// The mapping.
        name: MappingName,
    },
    /// Any other failure, with what `cryptsetup` printed.
    #[error("cryptsetup failed with status {status}: {output}")]
    Failed {
        /// The exit status, as a shell reports it.
        status: i32,
        /// Standard output and standard error, trimmed.
        output: String,
    },
    /// A device or mapping name starts with `-`: `cryptsetup` would read it
    /// as an option, so it is not run.
    #[error("{0:?} starts with '-', which cryptsetup would read as an option")]
    LeadingDash(String),
}

impl CryptError {
    /// The exit status that `cryptsetup` ended with, as a shell reports it:
    /// 127 when it could not be started or is missing, 1 when it was not
    /// run.
    pub fn status(&self) -> i32 {
        match self {
            CryptError::Start(_) | CryptError::NotInstalled => 127,
            CryptError::WrongKey { .. } => 2,
            CryptError::NoDevice { .. } => 4,
            CryptError::Busy { .. } => 5,
            CryptError::Failed { status, .. } => *status,
            CryptError::LeadingDash(_) => 1,
        }
    }
}

zbm_types::error_codes! {
    CryptError, CRYPT_CODES;
    Start = 501 => "cryptsetup could not be started":
        "cryptsetup is in the image but could not be run; the system may be out of memory or \
         processes.",
    NotInstalled = 502 => "cryptsetup is not installed":
        "The image has no cryptsetup, so no LUKS volume can be opened. Rebuild the image with LUKS \
         support.",
    WrongKey = 503 => "no key slot accepts the key":
        "The passphrase or key was wrong for every key slot of the volume. Check the key file, the \
         keyboard layout of the console, or the token that gave the key.",
    NoDevice = 504 => "the device is not a LUKS volume":
        "The device does not exist, or has no LUKS header. Check the device in crypttab, and that \
         its controller's driver is in the image.",
    Busy = 505 => "the mapping exists already or is busy":
        "A mapping of that name is open already, perhaps from an earlier unlock. Close it with \
         cryptsetup close, or choose another name.",
    Failed = 506 => "cryptsetup failed":
        "cryptsetup ended with an error, whose output follows the code.",
    LeadingDash = 507 => "a name would be read as an option":
        "A device or mapping name starts with '-', which cryptsetup would read as an option. \
         Rename it.",
}
