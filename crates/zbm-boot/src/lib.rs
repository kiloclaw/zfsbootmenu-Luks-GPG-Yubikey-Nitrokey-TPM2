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
#![warn(unreachable_pub)]

mod boot;
mod error;
pub mod graphics;
mod load;
mod recovery;
pub mod saved_logs;

#[cfg(test)]
mod testing;

pub use boot::{BootSettings, export_pool, kexec_kernel, mount_efivarfs, mount_securityfs};
pub use error::{BOOT_CODES, BootError};
pub use load::KexecLoad;
pub use recovery::Recovery;

/// Measured boot (ROADMAP.md SE10, ADR 0008): what the boot measures into
/// the TPM, the event log and the prediction of the values the PCRs take.
pub mod measured {
    pub use crate::boot::measured::{
        Bank, COMMAND_LINE_PCR, Digests, Event, Extended, FILES_PCR, Measured, extend_events,
        has_tpm, hex, measure, parse_log, predicted, replay, unsealed_at,
    };
}
