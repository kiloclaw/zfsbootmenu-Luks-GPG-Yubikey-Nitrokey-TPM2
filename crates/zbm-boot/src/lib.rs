//! Booting a boot environment in the ZFSBootMenu runtime (ADR 0015): the
//! sequence from a chosen kernel to `kexec`, the `kexec` load, measured
//! boot, the teardown, the GPUs handed back before `kexec`, the logs saved
//! before the boot, `efivarfs` and `securityfs`, and the recovery port. What
//! a boot environment is (its datasets, kernels, command line, keys and
//! hooks) is `zbm-bootenv`'s.
//!
//! Every function reaches the system through `zbm_platform::Context`, asks
//! the person through the `Interaction` it is given, and shares state only
//! through `zbm_state::State`.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]

pub mod boot;
pub mod error;
pub mod graphics;
pub mod load;
pub mod recovery;
pub mod saved_logs;

#[cfg(test)]
mod testing;
pub use error::{BOOT_CODES, BootError};
