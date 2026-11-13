//! Mounting and unmounting: a boot environment on its mount point in the
//! state directory, and the recursive unmount of a mount point.

use std::path::Path;

use zbm_platform::{Context, Files};
use zbm_state::State;
use zbm_tools::Shown;
use zbm_tools::mount::{Mount, Mounted};
use zbm_types::{DatasetName, FilesystemName};
use zbm_zfs::{Stderr, ZfsText};

use crate::error::BootEnvError;
use crate::zfs::locked_root;

/// Creates the state directory of a boot environment if needed. Its
/// paths, the mount point among them, come from [`State`].
pub(super) fn be_location(state: &State<'_>, fs: &str) {
    zbm_trace::debug!("fs set to {fs}"; %fs);
    // When `mkdir -p` fails, the paths are used all the same.
    let _ = state.environment(fs);
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
    context: &'a Context<'_>,
    state: &State<'_>,
    fs: &FilesystemName,
    noise: Noise,
) -> Option<Mounted<'a>> {
    mount_as(context, state, fs, noise, "ro")
}

/// Mounts the boot environment `dataset` read/write, on the mount point it
/// is mounted read-only on otherwise, as the legacy runtime did when asked
/// to; its pool must be imported read/write. A snapshot is never mounted
/// read/write.
pub fn mount_zfs_writable<'a>(
    context: &'a Context<'_>,
    state: &State<'_>,
    dataset: &DatasetName,
    noise: Noise,
) -> Option<Mounted<'a>> {
    mount_as(
        context,
        state,
        &FilesystemName::from(dataset.clone()),
        noise,
        "rw",
    )
}

/// Mounts `fs` on its mount point with `rwo`, `ro` or `rw`.
fn mount_as<'a>(
    context: &'a Context<'_>,
    state: &State<'_>,
    fs: &FilesystemName,
    noise: Noise,
    rwo: &str,
) -> Option<Mounted<'a>> {
    if locked_root(context.runner(), fs.dataset()).is_some() {
        zbm_trace::error!("{fs} is locked, unable to mount filesystem"; %fs);
        zbm_trace::debug!("{fs} is locked, returning"; %fs);
        return None;
    }
    let name = fs.to_string();
    be_location(state, &name);
    let mnt = state.mount_point(&name);
    let _ = context.files().create_dir_all(&mnt);
    let errors = match noise {
        Noise::Shown => Stderr::Console,
        Noise::Hidden => Stderr::Hidden,
    };
    let mountpoint = ZfsText::new(context.runner()).value(fs, "mountpoint", errors);
    let legacy = mountpoint.as_deref() == Some("legacy");
    let mnt_text = mnt.to_string_lossy();
    let zfsutil = !legacy && !fs.is_snapshot();
    if zfsutil {
        zbm_trace::debug!("mounting {fs} at {mnt_text} ({rwo}) with zfsutil"; %fs, %mnt_text, %rwo);
        zbm_trace::debug!("mount -o zfsutil,{rwo} -t zfs {fs} {mnt_text}"; %rwo, %fs, %mnt_text);
    } else {
        zbm_trace::debug!("mounting {fs} at {mnt_text} ({rwo})"; %fs, %mnt_text, %rwo);
    }
    let mount = Mount::new(context.runner(), context.files());
    let mounted = if rwo == "rw" {
        mount.zfs_writable(&name, &mnt, zfsutil, noise.shown())
    } else {
        mount.zfs(&name, &mnt, zfsutil, noise.shown())
    };
    let code = mounted.as_ref().err().copied().unwrap_or(0);
    zbm_trace::debug!("mount return code: {code}"; %code);
    mounted.ok()
}

/// Unmounts `path`, saying why it could not on the console.
pub fn umount(context: &Context<'_>, path: &Path) {
    let _ = Mount::new(context.runner(), context.files()).unmount(path, Shown::Console);
}

/// Unmounts `mountpoint` and everything below it with `umount -R`; when that
/// fails, unmounts each mount point that starts with it, deepest first.
///
/// # Errors
///
/// [`BootEnvError::Unmount`] with the mount points that stay mounted.
pub fn recursive_umount(context: &Context<'_>, mountpoint: &Path) -> Result<(), BootEnvError> {
    let mount = Mount::new(context.runner(), context.files());
    if mount.unmount_recursive(mountpoint) {
        return Ok(());
    }
    let below = mountpoint.to_string_lossy();
    let table = context
        .files()
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
        Err(BootEnvError::Unmount { mounts: mounted })
    }
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
