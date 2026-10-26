//! Booting a boot environment.
//!
//! Mounting a boot environment, finding its kernels, choosing one and
//! booting it with `kexec`, with the ZFS questions, mounts and unmounts this
//! needs, with read-only mounts. Keys of locked encryption roots are loaded
//! by `crate::keys` before the boot environment is mounted.
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

pub use discovery::{find_kernels, kernel_property, select_kernel};
pub use mounts::{Noise, is_mount_point, mount_efivarfs, mount_zfs, recursive_umount};
pub use sequence::{BootSettings, kexec_kernel};
pub(crate) use zfs::zfs_value;
pub use zfs::{encryption_root, export_pool, is_writable, locked_root};
