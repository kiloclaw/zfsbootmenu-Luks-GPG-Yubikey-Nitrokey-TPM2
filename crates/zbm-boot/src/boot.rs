//! Booting a boot environment.
//!
//! The Rust counterparts of `mount_zfs`, `find_be_kernels`, `select_kernel`,
//! `kexec_kernel` and the helpers they use (`be_has_encroot`, `be_is_locked`,
//! `is_writable`, `export_pool`, `recursive_umount`, `mount_efivarfs`), for
//! the default boot of the Phase 5 runtime: read-only mounts, no key loading
//! (a locked encryption root is not supported yet).
//!
//! The code is split by what it works on: `zfs` asks ZFS about datasets and
//! pools, `mounts` mounts and unmounts, `discovery` finds the kernels of a
//! boot environment, and `sequence` boots one of them. Everything public is
//! re-exported here.

mod discovery;
mod mounts;
mod sequence;
mod zfs;

#[cfg(test)]
mod tests;

pub use discovery::{find_kernels, select_kernel};
pub use mounts::{Noise, mount_efivarfs, mount_zfs, recursive_umount};
pub use sequence::{BootSettings, kexec_kernel};
pub(crate) use zfs::zfs_value;
pub use zfs::{encryption_root, export_pool, is_writable, locked_root};
