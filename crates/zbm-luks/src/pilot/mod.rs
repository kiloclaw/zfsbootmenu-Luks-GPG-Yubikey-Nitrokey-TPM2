//! Unlocking a volume by trying key methods in order.
//!
//! Each way of getting a key (a key file, a token, a prompt) is a
//! [`KeyMethod`]. An [`KeyChain`] tries its methods in order until one opens
//! the volume, the chain of responsibility of `docs/dev/architecture.md`; a
//! new method is one new type, and the order is data.
//!
//! A method answers with an [`Attempt`]: it opened the volume, it does not
//! apply ([`Unavailable`], such as a missing key file), or it tried and
//! `cryptsetup` refused ([`CryptError`]). When no method opens the volume,
//! [`UnlockFailed`] lists every attempt with its reason, so nothing is lost
//! between the method that failed and the user who reads the message.
//!
//! ```
//! use std::path::PathBuf;
//! use zbm_crypt::{Closed, Mapping, OpenOptions};
//! use zbm_luks::pilot::{KeyFile, KeyChain};
//! use zbm_platform::Context;
//! use zbm_testkit::fakes::FakeSystem;
//!
//! let fake = FakeSystem::new();
//! fake.files.file("/etc/keys/root.key", "secret");
//! let key_file = KeyFile::new(PathBuf::from("/etc/keys/root.key"));
//! let unlocker = KeyChain::new(vec![Box::new(key_file)]);
//! let (device, name) = ("/dev/sda2".parse()?, "cryptroot".parse()?);
//! let volume = Mapping::<Closed>::new(device, name);
//! let context = Context::new(fake.system());
//!
//! let unlocked = unlocker.unlock(&context, volume, OpenOptions::default())?;
//! assert_eq!(unlocked.method, "key file");
//! assert_eq!(unlocked.mapping.path(), "/dev/mapper/cryptroot");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod key_file;

pub use key_file::KeyFile;

use std::fmt;
use std::path::PathBuf;

use tracing::{info, info_span, warn};
use zbm_crypt::{Closed, CryptError, Mapping, Open, OpenOptions};
use zbm_platform::Context;

/// One way of getting the key of a volume and opening it.
pub trait KeyMethod {
    /// A short name for messages and the journal, such as `key file`.
    fn name(&self) -> &'static str;

    /// Tries to open `mapping` with `options`.
    fn try_open(
        &self,
        context: &Context<'_>,
        mapping: Mapping<Closed>,
        options: OpenOptions,
    ) -> Attempt;
}

/// What a [`KeyMethod`] did.
#[derive(Debug)]
pub enum Attempt {
    /// The volume is open.
    Opened(Mapping<Open>),
    /// The method does not apply to this volume; the mapping is unchanged.
    Unavailable {
        /// The mapping, still closed.
        mapping: Mapping<Closed>,
        /// Why the method does not apply.
        reason: Unavailable,
    },
    /// The method had a key and `cryptsetup` refused it or failed.
    Refused {
        /// The mapping, still closed.
        mapping: Mapping<Closed>,
        /// What `cryptsetup` reported.
        error: CryptError,
    },
}

/// Why a [`KeyMethod`] does not apply to a volume.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Unavailable {
    /// The key file does not exist.
    #[error("the key file {} does not exist", path.display())]
    KeyFileMissing {
        /// The key file.
        path: PathBuf,
    },
    /// The key file exists but cannot be read.
    #[error("the key file {} cannot be read", path.display())]
    KeyFileUnreadable {
        /// The key file.
        path: PathBuf,
        /// Why it cannot be read.
        #[source]
        source: std::io::Error,
    },
}

/// A volume opened by an [`KeyChain`].
#[derive(Debug)]
pub struct Unlocked {
    /// The open mapping.
    pub mapping: Mapping<Open>,
    /// The name of the method that opened it.
    pub method: &'static str,
}

/// Why one method did not open the volume.
#[derive(Debug)]
pub enum Refusal {
    /// The method does not apply.
    Unavailable(Unavailable),
    /// `cryptsetup` refused the method's key or failed.
    Refused(CryptError),
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Unavailable(reason) => write!(formatter, "{reason}"),
            Refusal::Refused(error) => write!(formatter, "{error}"),
        }
    }
}

/// No method opened the volume.
#[derive(Debug, thiserror::Error)]
#[error("no key method opened {}", mapping.device())]
pub struct UnlockFailed {
    /// The mapping, still closed.
    pub mapping: Mapping<Closed>,
    /// Each method that was tried, in order, with why it did not open the
    /// volume.
    pub attempts: Vec<(&'static str, Refusal)>,
}

/// Key methods, tried in order until one opens a volume.
pub struct KeyChain {
    /// The methods, in the order they are tried.
    methods: Vec<Box<dyn KeyMethod>>,
}

impl KeyChain {
    /// An unlocker that tries `methods` in this order.
    pub fn new(methods: Vec<Box<dyn KeyMethod>>) -> KeyChain {
        KeyChain { methods }
    }

    /// Opens `mapping` with the first method that can, each method in a
    /// `key_method` span.
    ///
    /// # Errors
    ///
    /// [`UnlockFailed`] with the closed mapping and every attempt when no
    /// method opened it.
    pub fn unlock(
        &self,
        context: &Context<'_>,
        mapping: Mapping<Closed>,
        options: OpenOptions,
    ) -> Result<Unlocked, UnlockFailed> {
        let _span =
            info_span!("unlock", device = %mapping.device(), name = %mapping.name()).entered();
        let mut mapping = mapping;
        let mut attempts = Vec::new();
        for method in &self.methods {
            let _method = info_span!("key_method", method = method.name()).entered();
            match method.try_open(context, mapping, options) {
                Attempt::Opened(open) => {
                    info!("opened");
                    return Ok(Unlocked {
                        mapping: open,
                        method: method.name(),
                    });
                }
                Attempt::Unavailable {
                    mapping: closed,
                    reason,
                } => {
                    info!(%reason, "not available");
                    attempts.push((method.name(), Refusal::Unavailable(reason)));
                    mapping = closed;
                }
                Attempt::Refused {
                    mapping: closed,
                    error,
                } => {
                    warn!(%error, "refused");
                    attempts.push((method.name(), Refusal::Refused(error)));
                    mapping = closed;
                }
            }
        }
        Err(UnlockFailed { mapping, attempts })
    }
}

#[cfg(test)]
mod tests;
