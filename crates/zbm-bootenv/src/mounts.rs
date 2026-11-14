//! Mounting and unmounting: a boot environment on its mount point in the
//! state directory, only where that path, the state directory included,
//! passes through no link and no other mount and holds no other
//! filesystem ([`refusal`]), and the recursive unmount of a mount point.

use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use zbm_platform::{Context, Files};
use zbm_state::State;
use zbm_tools::Shown;
use zbm_tools::mount::inside::{InRoot, Obstacle};
use zbm_tools::mount::table::{MOUNTINFO, mounts};
use zbm_tools::mount::{Mount, Mounted};
use zbm_types::{DatasetName, FilesystemName, Printable};
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

/// Why `fs` is not to be mounted on `at`, its mount point, if it is not:
/// the kernel's table of mounts could not be read; `at` is not below the
/// state directory; the state directory itself is a symbolic link, could
/// not be read, or is a mount point ([`base_obstacle`]); a name on the way
/// from it is a symbolic link or could not be read, or a directory on the
/// way is a mount point ([`InRoot::first_obstacle`]); or another
/// filesystem is mounted on `at` itself, which `fs` would hide and a
/// recursive unmount of `at` would take. `fs` itself on `at`, left mounted
/// there, is no reason: it is mounted again over it, as before.
fn refusal(files: &dyn Files, state: &State<'_>, fs: &FilesystemName, at: &Path) -> Option<String> {
    let table = match files.read(Path::new(MOUNTINFO)) {
        Ok(table) => table,
        Err(error) => return Some(format!("{MOUNTINFO} could not be read: {error}")),
    };
    let base = state.base();
    let Some(walk) = at.strip_prefix(base).ok().and_then(InRoot::new) else {
        return Some(format!("{} is not below {}", at.display(), base.display()));
    };
    let obstacle =
        base_obstacle(files, base, &table).or_else(|| walk.first_obstacle(files, base, &table));
    if let Some(obstacle) = obstacle {
        return Some(obstacle.to_string());
    }
    let (point, name) = (at.as_os_str().as_bytes(), fs.to_string());
    let foreign = mounts(&table).any(|listed| {
        listed.point == point && (listed.kind != b"zfs" || listed.source != name.as_bytes())
    });
    foreign.then(|| Obstacle::Mount(at.to_path_buf()).to_string())
}

/// Why the state directory `base` cannot hold the state's mounts, if it
/// cannot: it is a symbolic link, or could not be read, or `table` lists
/// it as a mount point, as of a dataset whose `mountpoint` property it is.
/// Nothing in the image mounts on it. A missing `base` is no obstacle:
/// the directories are made, with no link on the way, before the mount.
fn base_obstacle(files: &dyn Files, base: &Path, table: &[u8]) -> Option<Obstacle> {
    match files.read_link(base) {
        Ok(target) => return Some(Obstacle::Link(base.to_path_buf(), target)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::InvalidInput
                    | io::ErrorKind::NotFound
                    | io::ErrorKind::NotADirectory
            ) => {}
        Err(error) => return Some(Obstacle::Unread(error)),
    }
    let point = base.as_os_str().as_bytes();
    mounts(table)
        .any(|listed| listed.point == point)
        .then(|| Obstacle::Mount(base.to_path_buf()))
}

/// Mounts `fs` on its mount point with `rwo`, `ro` or `rw`, unless
/// [`refusal`] gives a reason, which an error line says; nothing is
/// created or mounted then.
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
    let mnt = state.mount_point(&name);
    if let Some(why) = refusal(context.files(), state, fs, &mnt) {
        let why = Printable(&why).to_string();
        zbm_trace::error!("{fs} is not mounted: {why}"; %fs, %why);
        return None;
    }
    be_location(state, &name);
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
/// fails, unmounts each mount point at or below it, deepest first, as the
/// kernel's table of mounts lists them. Names are compared whole and
/// unescaped, so that `/mnt/be` takes neither `/mnt/bee` nor
/// `/mnt/be@s/mnt`, and a name with a space is found.
///
/// # Errors
///
/// [`BootEnvError::Unmount`] with the mount points that stay mounted.
pub fn recursive_umount(context: &Context<'_>, mountpoint: &Path) -> Result<(), BootEnvError> {
    let mount = Mount::new(context.runner(), context.files());
    if mount.unmount_recursive(mountpoint) {
        return Ok(());
    }
    let table = context
        .files()
        .read(Path::new(MOUNTINFO))
        .unwrap_or_default();
    let mut below: Vec<(usize, PathBuf)> = mounts(&table)
        .map(|listed| PathBuf::from(OsStr::from_bytes(&listed.point)))
        .filter(|point| point.starts_with(mountpoint))
        .map(|point| (point.components().count(), point))
        .collect();
    // sort -n -k1 -r on "depth,path"; a path listed twice, with one
    // filesystem stacked on another, is unmounted twice.
    below.sort_by(|a, b| b.cmp(a));
    let mut mounted = Vec::new();
    for (_, point) in below {
        if !mount.unmount(&point, Shown::Hidden) {
            let mp = point.to_string_lossy().into_owned();
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

#[cfg(test)]
mod tests;
