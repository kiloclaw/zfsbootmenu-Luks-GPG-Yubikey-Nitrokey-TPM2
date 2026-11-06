//! Booting a boot environment: the sequence from a chosen kernel to
//! `kexec` (`sequence`), measured boot (`measured`), the logs and the
//! record saved before the teardown (`record`), the kernel's file systems
//! it needs (`mounts`) and the export of pools before the new kernel runs
//! (`zfs`). The crate's root re-exports what other crates use,
//! measured boot's items in its module `measured`.

pub(crate) mod measured;
mod mounts;
mod record;
mod sequence;
mod zfs;

#[cfg(test)]
mod tests;

pub use mounts::{mount_efivarfs, mount_securityfs};
pub use sequence::{BootSettings, kexec_kernel};
pub use zfs::export_pool;
