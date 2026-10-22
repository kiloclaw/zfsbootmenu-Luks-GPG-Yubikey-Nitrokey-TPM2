//! LUKS volumes of the ZFSBootMenu runtime: package B of `PORT-ROADMAP.md`.
//!
//! This crate replaces `lib/zfsbootmenu-luks.sh` and
//! `lib/zfsbootmenu-luks-keys.sh`:
//!
//! - the configuration: the embedded crypttab, `zbm.luks` and `zbm.luks_key`
//!   ([`config`]);
//! - finding volumes, deciding which may hold ZFS, waiting for devices, and
//!   volumes that appear late ([`device`]);
//! - the key methods, in the legacy order: key scripts, key files and GnuPG key
//!   files, LUKS2 tokens (TPM2, FIDO2, clevis), passphrases already entered,
//!   YubiKey challenge-response, and the passphrase prompt;
//! - closing volumes and forgetting secrets before booting, and `zluks`.
//!
//! Operations that can fail return [`error::LuksError`]; those that end in one
//! of several good ways return an outcome, such as [`unlock::Opened`].
//!
//! Every secret is a `zbm_platform::Secret` and reaches programs only on
//! standard input (see "Secrets" in `crates/DESIGN.md`).
//!
//! An [`unlocker::Unlocker`] unlocks volumes: it tries the key methods of
//! [`unlock::methods`] in order, LUKS2 tokens through the handlers of
//! [`keys::handlers`], then the passphrase prompt of [`unlock::prompt`], and
//! keeps its records in a [`mappings::MappingStore`]. [`pilot`] is the key
//! file slice of R0 over the typed `cryptsetup` of `zbm-crypt`, which the
//! application layer's unlock use case moves onto the unlocker in R4.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]

pub mod close;
pub mod config;
pub mod crypto;
pub mod device;
pub mod error;
pub mod keyfile;
pub mod keys;
pub mod mappings;
pub mod pilot;
pub mod tokens;
pub mod unlock;
pub mod unlocker;

#[cfg(test)]
mod testing;

pub use error::{LuksError, TokenFailure};
