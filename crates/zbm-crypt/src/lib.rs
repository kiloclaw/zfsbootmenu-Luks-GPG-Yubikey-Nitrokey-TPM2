//! Typed wrappers around the encryption programs ZFSBootMenu runs.
//!
//! This is an adapter in the sense of `docs/dev/architecture.md`: it turns a
//! request ("open this LUKS volume with this key") into a program run through
//! the `Runner` port, and the program's answer into Rust values. No other
//! crate builds `cryptsetup` command lines.
//!
//! A mapping is a [`Mapping<Closed>`](Mapping) until [`Cryptsetup::open`]
//! opens it and returns a [`Mapping<Open>`](Mapping); only an open mapping can
//! be closed, and closing hands back a closed one. The compiler therefore
//! rejects closing a volume that was never opened. A failure hands the
//! mapping back unchanged, in [`Failed`], with a [`CryptError`] that says
//! why.
//!
//! Keys reach `cryptsetup` only on standard input (`--key-file=-`), never as
//! an argument or a file (see "Secrets" in `crates/DESIGN.md`).
//!
//! ```
//! use zbm_crypt::{Closed, Cryptsetup, CryptError, Mapping, OpenOptions};
//! use zbm_platform::Secret;
//! use zbm_testkit::FakeRunner;
//!
//! let runner = FakeRunner::new();
//! let cryptsetup = Cryptsetup::new(&runner);
//! let (device, name) = ("/dev/sda2".parse()?, "cryptroot".parse()?);
//! let volume = Mapping::<Closed>::new(device, name);
//!
//! let open = cryptsetup
//!     .open(volume, &Secret::copy_from(b"key"), OpenOptions::default())
//!     .map_err(|failed| failed.error)?;
//! assert_eq!(open.path(), "/dev/mapper/cryptroot");
//! assert_eq!(
//!     runner.lines(),
//!     ["cryptsetup open --type luks --key-file=- /dev/sda2 cryptroot"]
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod cryptsetup;
mod mapping;
pub mod token;

pub use cryptsetup::{Cryptsetup, OpenOptions};
pub use mapping::{Closed, Failed, Mapping, Open};
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
}
