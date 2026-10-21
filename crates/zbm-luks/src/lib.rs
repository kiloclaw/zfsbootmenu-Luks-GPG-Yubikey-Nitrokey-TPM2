//! LUKS volumes of the ZFSBootMenu runtime: package B of `PORT-ROADMAP.md`.
//!
//! This crate will replace `lib/zfsbootmenu-luks.sh` and
//! `lib/zfsbootmenu-luks-keys.sh`:
//!
//! - the configuration: the embedded crypttab, `zbm.luks` and `zbm.luks_key`;
//! - finding volumes, deciding which may hold ZFS, waiting for devices, and
//!   volumes that appear late;
//! - the key methods, in the legacy order: key scripts, key files and GnuPG key
//!   files, LUKS2 tokens (TPM2, FIDO2, clevis), passphrases already entered,
//!   YubiKey challenge-response, and the passphrase prompt;
//! - closing volumes and forgetting secrets before booting, and `zluks`.
//!
//! Every secret is a `zbm_core::secret::Secret` and reaches programs only on
//! standard input (see "Secrets" in `crates/DESIGN.md`). The code arrives in
//! Phase 6; until then the crate only fixes its place in the layout.
