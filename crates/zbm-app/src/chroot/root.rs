//! The root of a chroot on its own path (SB16): `zbm_bootenv` mounts it
//! only where the path passes through no link and no other mount from the
//! state directory down (SB19); the root's line in the kernel's table is
//! read right after ([`Root::new`]); and the teardown unmounts by the path
//! only while no link or mount has appeared on its way
//! ([`InRoot::first_obstacle`]) and the root is there under no other mount
//! of its filesystem ([`Root::found`]), then makes sure by the root's mount
//! ID that nothing of it stays mounted ([`Root::tear_down`]), since
//! `umount -R` takes the topmost mount on a path, not the chroot's.

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use zbm_platform::Files;
use zbm_tools::mount::inside::InRoot;
use zbm_tools::mount::table::{Listed, MOUNTINFO, mounts};
use zbm_tools::mount::{Mount, Mounted};
use zbm_types::FilesystemName;

/// The most lazy unmounts of the root's path in one teardown.
const LAZY_ROUNDS: usize = 8;

/// What the teardown finds on the root's path.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum Found {
    /// The root, topmost on its path.
    Top,
    /// The root, under a mount that is no other root of its filesystem,
    /// such as one the chroot's shell made over its `/`.
    Under,
    /// Nothing of the root: something else unmounted it.
    Gone,
    /// The root, if it is there, where the kernel's table was not read:
    /// `umount -R` alone, as before.
    Unread,
    /// Why nothing on the path is to be unmounted by it.
    Left(String),
}

/// The root of a chroot, as its teardown finds it: where it is mounted,
/// below which state directory, the filesystem, and its line in the
/// kernel's table, read right after the mount.
pub(super) struct Root {
    /// The state directory, from which the teardown walks to the root.
    base: PathBuf,
    /// The mount point.
    at: PathBuf,
    /// The filesystem mounted there.
    fs: FilesystemName,
    /// Its line in the kernel's table; `None` where the table could not be
    /// read.
    line: Option<Listed>,
}

impl Root {
    /// `fs`, mounted on `at` below the state directory `base` just now,
    /// with its line: the last on `at` in the kernel's table, read before
    /// anything else is mounted and any code of the filesystem runs.
    pub(super) fn new(files: &dyn Files, base: &Path, at: PathBuf, fs: FilesystemName) -> Root {
        let point = at.as_os_str().as_bytes();
        let line = files
            .read(Path::new(MOUNTINFO))
            .ok()
            .and_then(|table| mounts(&table).filter(|listed| listed.point == point).last());
        Root {
            base: base.to_path_buf(),
            at,
            fs,
            line,
        }
    }

    /// The mount point.
    pub(super) fn at(&self) -> &Path {
        &self.at
    }

    /// What is on the root's path now: [`Found::Left`] where the path is
    /// not below the state directory, or a link or a mount on its way has
    /// appeared since the root was mounted ([`InRoot::first_obstacle`]),
    /// or where the topmost mount on it is another root of the filesystem
    /// (another chroot's, or the boot path's), which is not this chroot's
    /// to unmount. The links are walked where the kernel's table could not
    /// be read too; it is [`Found::Unread`] then.
    pub(super) fn found(&self, files: &dyn Files) -> Found {
        let table = files.read(Path::new(MOUNTINFO));
        let Some(walk) = self.at.strip_prefix(&self.base).ok().and_then(InRoot::new) else {
            let (at, base) = (self.at.display(), self.base.display());
            return Found::Left(format!("{at} is not below {base}"));
        };
        let listed = table.as_deref().unwrap_or_default();
        if let Some(obstacle) = walk.first_obstacle(files, &self.base, listed) {
            return Found::Left(obstacle.to_string());
        }
        let Some(ours) = &self.line else {
            return Found::Unread;
        };
        let Ok(table) = table else {
            return Found::Unread;
        };
        let point = self.at.as_os_str().as_bytes();
        let on_it: Vec<Listed> = mounts(&table)
            .filter(|listed| listed.point == point)
            .collect();
        if !on_it.iter().any(|listed| listed.id == ours.id) {
            return Found::Gone;
        }
        match on_it.last() {
            Some(top) if top.id == ours.id => Found::Top,
            Some(top) if top.is_mounted_as(ours) => {
                Found::Left(format!("another mount of {} is on it", self.fs))
            }
            _ => Found::Under,
        }
    }

    /// Why the chroot's mounts are to be left as they are, if they are:
    /// [`Found::Left`]'s reason, or the root is gone, and with it what was
    /// mounted below it.
    pub(super) fn why_left(&self, files: &dyn Files) -> Option<String> {
        match self.found(files) {
            Found::Left(why) => Some(why),
            Found::Gone => Some("it is no longer mounted".to_owned()),
            Found::Top | Found::Under | Found::Unread => None,
        }
    }

    /// Unmounts `root`, the chroot's root, with everything on and below it
    /// (`umount -R`), where [`Root::found`] finds it; then, unless that
    /// took the root itself, detaches what is left ([`Root::detach`]). A
    /// warning says when the root is left, when anything was detached
    /// lazily, and when the root stays mounted.
    pub(super) fn tear_down(&self, mount: Mount<'_>, files: &dyn Files, root: Mounted<'_>) {
        let path = self.at.display();
        let top = match self.found(files) {
            Found::Top => true,
            Found::Under => false,
            Found::Unread => {
                let _ = root.unmount();
                return;
            }
            Found::Gone => {
                let _ = root.leave();
                return;
            }
            Found::Left(why) => {
                let _ = root.leave();
                tracing::warn!(%path, why, "the chroot's root is left mounted");
                return;
            }
        };
        // Taken whole: its ID is free, so the table is not read again.
        if root.unmount() && top {
            return;
        }
        let (detached, stays) = self.detach(mount, files);
        if detached > 0 {
            tracing::warn!(%path, detached, "the chroot's root was detached lazily");
        }
        if let Some(why) = stays {
            tracing::warn!(%path, why, "the chroot's root stays mounted");
        }
    }

    /// Detaches the topmost mount on the root's path with everything on
    /// and below it (`umount -l`), while [`Root::found`] finds the root
    /// there, at most [`LAZY_ROUNDS`] times, which takes an overmount, a
    /// busy mount, and at last the root itself. Once the root itself is
    /// detached, the table is not read again: the kernel gives its ID to
    /// the next mount. How many it detached, and why the root stays
    /// mounted, if it does.
    fn detach(&self, mount: Mount<'_>, files: &dyn Files) -> (usize, Option<String>) {
        let mut detached = 0;
        for _ in 0..LAZY_ROUNDS {
            let top = match self.found(files) {
                Found::Top => true,
                Found::Under => false,
                Found::Gone => return (detached, None),
                Found::Unread => return (detached, Some("the table was not read".to_owned())),
                Found::Left(why) => return (detached, Some(why)),
            };
            if mount.unmount_lazy(&self.at).is_ok() {
                detached += 1;
                if top {
                    return (detached, None);
                }
            }
        }
        let why = format!("it is listed after {LAZY_ROUNDS} lazy unmounts");
        (detached, Some(why))
    }
}
