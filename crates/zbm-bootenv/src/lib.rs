//! Boot environments of the ZFSBootMenu runtime (ADR 0015): what ZFS says
//! of a boot environment's dataset, mounting it, its kernels, its command
//! line, where its key comes from and loading it, and the hooks, which key
//! loading runs too. `zbm-boot` boots one.
//!
//! - [`kernels`]: the kernels and initramfs images of a boot environment;
//! - [`commandline`]: its command line, its root prefix and device tree;
//! - [`keys`]: key sources, the key cache, and loading a key, which asks
//!   the person through the `Interaction` it is given;
//! - [`hooks`]: the hook directories and running a stage of them.
//!
//! Every function reaches the system through `zbm_platform::Context` and
//! shares state only through `zbm_state::State`.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]

pub mod commandline;
mod discovery;
pub mod error;
pub mod hooks;
pub mod kernels;
pub mod keys;
mod mounts;
mod zfs;

#[cfg(test)]
mod testing;

pub use discovery::{find_kernels, kernel_property, select_kernel};
pub use error::{BOOTENV_CODES, BootEnvError};
pub use mounts::{Noise, is_mount_point, mount_zfs, recursive_umount, umount};
pub use zfs::{encryption_root, locked_root, zfs_value};
