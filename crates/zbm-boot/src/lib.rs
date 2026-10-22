//! Start-up, pool import and booting of the ZFSBootMenu runtime: packages A, C
//! and D of `PORT-ROADMAP.md`, and the minimal runtime of Phase 5.
//!
//! This crate will replace the start-up stages in `init.d/` and most of
//! `lib/zfsbootmenu-core.sh`:
//!
//! - the start-up sequence, in the order "Processes" in `crates/DESIGN.md`
//!   lists: kernel modules, the host ID, waiting for devices, hooks, LUKS
//!   unlocking (through `zbm-luks`) and pool import with its policies and
//!   retries;
//! - running hooks with their environment;
//! - boot environments, their kernels and command lines, ZFS encryption keys
//!   and key sources, and booting with `kexec`;
//! - snapshots, clones, rollbacks, checkpoints, read-write imports and chroot.
//!
//! Every function reaches the system through `zbm_platform::System` and shares
//! state only through `zbm_state::State`. The code arrives in phases 5 and 6.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]

pub mod boot;
pub mod commandline;
pub mod error;
pub mod hooks;
pub mod import;
pub mod kernels;
pub mod prompt;
pub mod recovery;
pub mod resume;
pub mod shadow;
pub mod support;

#[cfg(test)]
mod testing;
pub use error::BootError;
