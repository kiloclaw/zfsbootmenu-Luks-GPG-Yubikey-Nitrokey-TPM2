//! Booting a boot environment: the sequence from a chosen kernel to
//! `kexec` (`sequence`), measured boot (`measured`), the kernel's file
//! systems it needs (`mounts`) and the export of pools before the new
//! kernel runs (`zfs`). Everything public is re-exported here.

pub(crate) mod measured;
mod mounts;
mod sequence;
mod zfs;

#[cfg(test)]
mod tests;

pub use mounts::{mount_efivarfs, mount_securityfs};
pub use sequence::{BootSettings, kexec_kernel};
pub use zfs::export_pool;
