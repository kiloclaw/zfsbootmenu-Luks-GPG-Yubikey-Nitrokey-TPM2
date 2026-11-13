//! The kernel's file systems in a chroot, as `zfs-chroot` mounted them:
//! this system's `/tmp`, a `proc`, a `sysfs` and this system's `/dev`
//! below the root; on EFI, an `efivarfs` on the chroot's `sysfs`; and a
//! `devpts` on its `/dev`. Each mount point is walked right before its
//! mount ([`obstacle`]): one that is a symbolic link, or passes through
//! one, is not mounted, and a notice says why. A mount that fails is left
//! out, as `zfs-chroot` left it.

use std::path::{Path, PathBuf};

use zbm_tools::mount::{HostEfivars, Mount, Mounted};

use super::{Mounts, not_mounted, notice, obstacle};

/// The directories below the root that the kernel's file systems and
/// this system's cover, in the order mounted.
pub(super) const MOUNT_POINTS: [&str; 4] = ["tmp", "proc", "sys", "dev"];

/// Where this system's EFI variables are, and where a chroot's are below
/// its root.
const EFIVARS: &str = "/sys/firmware/efi/efivars";

/// Mounts the kernel's file systems with `mount` below the root at `at`,
/// in the chroot's mode, read/write when `writable`, into `mounts`, with
/// a notice for each one refused: `/tmp`, which `mounts` records,
/// `/proc`, `/sys` and `/dev`; then, on EFI, an `efivarfs` only where the
/// chroot's `sysfs` is mounted, and a `devpts` only where `/dev` is bound
/// and has `pts`.
pub(super) fn mount<'c>(mount: Mount<'c>, at: &Path, writable: bool, mounts: &mut Mounts<'c>) {
    let [tmp, proc, sys, dev] = MOUNT_POINTS;
    let bound_tmp = on(mounts, at, tmp, |target| {
        mount.bind(Path::new("/tmp"), target)
    });
    mounts.tmp = bound_tmp.is_some();
    on(mounts, at, proc, |target| mount.proc(target));
    let sysfs = on(mounts, at, sys, |target| mount.sysfs(target));
    let bound_dev = on(mounts, at, dev, |target| {
        mount.bind(Path::new("/dev"), target)
    });
    if sysfs.is_some() {
        efivarfs(mount, at, writable, mounts);
    }
    if bound_dev.is_some()
        && let Some(pts) = allowed(mounts, at, "dev/pts", "/dev/pts")
        && mounts.files.is_dir(&pts)
        && let Ok(devpts) = mount.devpts(&pts)
    {
        mounts.below.push(devpts);
    }
}

/// Mounts with `how` on `path` below the root at `at` where nothing is in
/// the way ([`allowed`]), into `mounts`: the mount point, where it is
/// mounted.
fn on<'c>(
    mounts: &mut Mounts<'c>,
    at: &Path,
    path: &str,
    how: impl FnOnce(&Path) -> Result<Mounted<'c>, i32>,
) -> Option<PathBuf> {
    let target = allowed(mounts, at, path, &format!("/{path}"))?;
    match how(&target) {
        Ok(mounted) => {
            mounts.below.push(mounted);
            Some(target)
        }
        Err(code) => {
            tracing::debug!(code, path, "not mounted in the chroot");
            None
        }
    }
}

/// `path` below the root at `at`, where nothing is in the way of a mount
/// on it ([`obstacle`]); `None`, and the notice that `what` is not
/// mounted and why in `mounts`, otherwise.
fn allowed(mounts: &mut Mounts<'_>, at: &Path, path: &str, what: &str) -> Option<PathBuf> {
    let target = at.join(path);
    match obstacle(mounts.files, at, &target) {
        None => Some(target),
        Some(why) => {
            tracing::debug!(path, why, "refused in the chroot");
            mounts.notices.push(not_mounted(what, &why));
            None
        }
    }
}

/// On EFI, this system's `efivarfs` remounted in the chroot's mode, or
/// mounted so where it is not, and one on the chroot's `sysfs` below the
/// root at `at`, into `mounts`. This system's is left alone when the
/// chroot's mount point is refused, and set back to read-only at once,
/// before the shell, when the chroot's does not mount.
fn efivarfs<'c>(mount: Mount<'c>, at: &Path, writable: bool, mounts: &mut Mounts<'c>) {
    let files = mounts.files;
    if !files.is_dir(Path::new("/sys/firmware/efi")) {
        return;
    }
    let Some(inside) = allowed(mounts, at, EFIVARS.trim_start_matches('/'), "efivarfs") else {
        return;
    };
    let mode = if writable { "rw" } else { "ro" };
    let host = if zbm_bootenv::is_mount_point(files, EFIVARS) {
        HostEfivars::remount(mount, mode)
    } else {
        HostEfivars::mount(mount, mode)
    };
    let Ok(host) = host else {
        return;
    };
    match mount.efivarfs_in(&inside, mode) {
        Ok(efivarfs) => {
            mounts.below.push(efivarfs);
            mounts.notices.push(notice("efivarfs", writable));
            mounts.host = Some(host);
        }
        Err(code) => {
            tracing::debug!(
                code,
                "no efivarfs in the chroot: this system's is read-only again"
            );
            drop(host);
        }
    }
}
