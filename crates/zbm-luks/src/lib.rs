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
//! Every secret is a `zbm_core::secret::Secret` and reaches programs only on
//! standard input (see "Secrets" in `crates/DESIGN.md`).
//!
//! [`unlocker`] is the pilot of REFACTOR-ROADMAP.md (R0): key methods as
//! strategies over the typed `cryptsetup` of `zbm-crypt`, which R3 grows into
//! the replacement of [`session`].

pub mod close;
pub mod config;
pub mod crypto;
pub mod device;
pub mod error;
pub mod keyfile;
pub mod keys;
pub mod mappings;
pub mod session;
pub mod tokens;
pub mod unlock;
pub mod unlocker;

#[cfg(test)]
mod testing;

pub use error::{LuksError, TokenFailure};
