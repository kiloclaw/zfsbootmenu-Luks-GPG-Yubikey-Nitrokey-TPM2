//! Mounting and unmounting: a boot environment on its mount point in the
//! state directory, the recursive unmount of a mount point, and `efivarfs`.

use std::path::{Path, PathBuf};

use zbm_platform::{Files, System};
use zbm_state::State;
use zbm_tools::Shown;
use zbm_tools::mount::{Mount, Mounted};
use zbm_types::FilesystemName;
use zbm_zfs::{Stderr, ZfsText};

use super::zfs::locked_root;
use crate::error::BootError;

/// The state directory of a boot environment, created if needed.
pub(super) fn be_location(state: &State<'_>, fs: &str) -> PathBuf {
    zbm_trace::debug!("fs set to {fs}"; %fs);
    // When `mkdir -p` fails, be_location gives the path all the same.
    state.environment(fs).unwrap_or_else(|_| {
        let mnt = state.mount_point(fs);
        mnt.parent().map_or(mnt.clone(), Path::to_path_buf)
    })
}

/// How the output of `mount` reaches the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Noise {
    /// Error messages go to the console, as when booting.
    Shown,
    /// Error messages are captured and dropped, as when looking for kernels.
    Hidden,
}

impl Noise {
    /// Where `mount`'s error messages go.
    fn shown(self) -> Shown {
        match self {
            Noise::Shown => Shown::Console,
            Noise::Hidden => Shown::Hidden,
        }
    }
}

/// Mounts `fs` read-only on its mount point in the state directory, which
/// it stays mounted on until the [`Mounted`] is dropped. A locked boot
/// environment is not mounted; one whose `mountpoint` is not `legacy`, and
/// that is not a snapshot, is mounted with `zfsutil`.
pub fn mount_zfs<'a>(
    system: &System<'a>,
    state: &State<'_>,
    fs: &FilesystemName,
    noise: Noise,
) -> Option<Mounted<'a>> {
    if locked_root(system.runner, fs.dataset()).is_some() {
        zbm_trace::error!("{fs} is locked, unable to mount filesystem"; %fs);
        zbm_trace::debug!("{fs} is locked, returning"; %fs);
        return None;
    }
    let name = fs.to_string();
    let mnt = be_location(state, &name).join("mnt");
    let _ = system.files.create_dir_all(&mnt);
    let rwo = "ro";

    let errors = match noise {
        Noise::Shown => Stderr::Console,
        Noise::Hidden => Stderr::Hidden,
    };
    let mountpoint = ZfsText::new(system.runner).value(fs, "mountpoint", errors);
    let legacy = mountpoint.as_deref() == Some("legacy");
    let mnt_text = mnt.to_string_lossy();
    let zfsutil = !legacy && !fs.is_snapshot();
    if zfsutil {
        zbm_trace::debug!("mounting {fs} at {mnt_text} ({rwo}) with zfsutil"; %fs, %mnt_text, %rwo);
        zbm_trace::debug!("mount -o zfsutil,{rwo} -t zfs {fs} {mnt_text}"; %rwo, %fs, %mnt_text);
    } else {
        zbm_trace::debug!("mounting {fs} at {mnt_text} ({rwo})"; %fs, %mnt_text, %rwo);
    }
    let mounted = Mount::new(system.runner, system.files).zfs(&name, &mnt, zfsutil, noise.shown());
    let code = mounted.as_ref().err().copied().unwrap_or(0);
    zbm_trace::debug!("mount return code: {code}"; %code);
    mounted.ok()
}

/// Unmounts `path`, saying why it could not on the console.
pub(super) fn umount(system: &System<'_>, path: &Path) {
    let _ = Mount::new(system.runner, system.files).unmount(path, Shown::Console);
}

/// Unmounts `mountpoint` and everything below it with `umount -R`; when that
/// fails, unmounts each mount point that starts with it, deepest first.
///
/// # Errors
///
/// [`BootError::Unmount`] with the mount points that stay mounted.
pub fn recursive_umount(system: &System<'_>, mountpoint: &Path) -> Result<(), BootError> {
    let mount = Mount::new(system.runner, system.files);
    if mount.unmount_recursive(mountpoint) {
        return Ok(());
    }
    let below = mountpoint.to_string_lossy();
    let table = system
        .files
        .read_to_string(Path::new("/proc/self/mounts"))
        .unwrap_or_default();
    let mut mounts: Vec<(usize, String)> = table
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(str::to_owned))
        .filter(|mp| mp.starts_with(&*below))
        .map(|mp| (mp.matches('/').count(), mp))
        .collect();
    // sort -n -k1 -r on "depth,path"
    mounts.sort_by(|a, b| b.cmp(a));
    let mut mounted = Vec::new();
    for (_, mp) in mounts {
        if !mount.unmount(Path::new(&mp), Shown::Hidden) {
            zbm_trace::error!("unable to unmount {mp}"; %mp);
            mounted.push(mp);
        }
    }
    if mounted.is_empty() {
        Ok(())
    } else {
        Err(BootError::Unmount { mounts: mounted })
    }
}

/// Whether the machine booted with UEFI: `/sys/firmware/efi` exists.
fn is_efi_system(system: &System<'_>) -> bool {
    system.files.is_dir(Path::new("/sys/firmware/efi"))
}

/// Whether something is mounted on `path`, as the kernel's table of this
/// process's mounts says; the images have no `mountpoint` program.
pub fn is_mount_point(files: &dyn Files, path: &str) -> bool {
    files
        .read_to_string(Path::new("/proc/self/mounts"))
        .unwrap_or_default()
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(path))
}

/// Mounts or remounts `efivarfs` read-only (`ro`) or read-write (`rw`) on
/// `/sys/firmware/efi/efivars`. Does nothing without UEFI.
pub fn mount_efivarfs(system: &System<'_>, mode: &str) {
    let location = "/sys/firmware/efi/efivars";
    if !is_efi_system(system) {
        zbm_trace::debug!("efivarfs unsupported");
        return;
    }
    let mount = Mount::new(system.runner, system.files);
    let mounted = is_mount_point(system.files, location);
    let options = if mounted {
        zbm_trace::debug!("remounting '{location}' '{mode}'"; %location, %mode);
        format!("remount,{mode}")
    } else {
        zbm_trace::debug!("mounting '{location}' '{mode}'"; %location, %mode);
        mode.to_owned()
    };
    let _ = mount.efivarfs(location, &options);
}
