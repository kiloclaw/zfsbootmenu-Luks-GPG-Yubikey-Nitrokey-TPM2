//! Restoring paths of a boot environment from one of its snapshots, on the
//! machine: each path is copied from the snapshot's read-only directory
//! below the mount, `.zfs/snapshot/<name>`, with `cp -a` to a new name
//! beside it, which then replaces the path. Nothing is deleted.
//!
//! No symbolic link is followed. One in a directory above a path, in the
//! environment or in the snapshot, could lead out of the environment into
//! ZFSBootMenu's own root, so such a path is refused before anything is
//! written; the path itself is replaced by the copy, never written
//! through. The names of the files, and what `cp` says of them, are shown
//! only as [`Printable`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use zbm_platform::{Files, Runner};
use zbm_tools::programs;
use zbm_types::{Printable, SnapshotName};

/// What a restore copies from and into.
pub(super) struct Restore<'a> {
    /// The files of the machine.
    files: &'a dyn Files,
    /// What runs `cp`.
    runner: &'a dyn Runner,
    /// The snapshot.
    snapshot: &'a SnapshotName,
    /// The environment, mounted read/write.
    into: PathBuf,
    /// The snapshot's files, below the mount.
    from: PathBuf,
}

impl<'a> Restore<'a> {
    /// The restore from `snapshot` into its environment, mounted read/write
    /// at `mounted`, with the files and programs of `files` and `runner`.
    pub fn new(
        files: &'a dyn Files,
        runner: &'a dyn Runner,
        mounted: &str,
        snapshot: &'a SnapshotName,
    ) -> Restore<'a> {
        let into = PathBuf::from(mounted);
        let from = into.join(".zfs/snapshot").join(snapshot.name());
        Restore {
            files,
            runner,
            snapshot,
            into,
            from,
        }
    }

    /// Fails, with the reason for each path, unless each of `paths` is in
    /// the snapshot, no directory above it is a symbolic link, in the
    /// environment or in the snapshot, and it is a directory in both or in
    /// neither, as a copy can only replace what is of its kind.
    pub fn check(&self, paths: &[String]) -> Result<(), Vec<String>> {
        if !self.files.is_dir(&self.from) {
            return Err(vec![format!(
                "the files of {} cannot be read",
                self.snapshot
            )]);
        }
        let why: Vec<String> = paths.iter().filter_map(|path| self.refusal(path)).collect();
        if why.is_empty() { Ok(()) } else { Err(why) }
    }

    /// Why `path` cannot be restored, if it cannot.
    fn refusal(&self, path: &str) -> Option<String> {
        let environment = self.snapshot.dataset();
        for above in directories_above(path) {
            if is_link(self.files, &below(&self.into, above)) {
                let above = Printable(above);
                return Some(format!("{above} is a symbolic link in {environment}"));
            }
            if is_link(self.files, &below(&self.from, above)) {
                let above = Printable(above);
                return Some(format!("{above} is a symbolic link in {}", self.snapshot));
            }
        }
        let (source, target) = (below(&self.from, path), below(&self.into, path));
        let shown = Printable(path);
        if !is_there(self.files, &source) {
            return Some(format!("{shown} is not in {}", self.snapshot));
        }
        let is_source_directory = is_directory(self.files, &source);
        if is_there(self.files, &target) && is_source_directory != is_directory(self.files, &target)
        {
            let (directory, other) = if is_source_directory {
                (self.snapshot.to_string(), environment.to_string())
            } else {
                (environment.to_string(), self.snapshot.to_string())
            };
            return Some(format!(
                "{shown} is a directory in {directory} but not in {other}"
            ));
        }
        None
    }

    /// Copies `paths` back in their order, parents first; how many were
    /// restored. A directory the environment has is left as it is, and
    /// counts as restored: what changed below it are paths of their own.
    ///
    /// # Errors
    ///
    /// Why a path was not copied, and how many were before it; those after
    /// it are not tried.
    pub fn copy(&self, paths: &[String]) -> Result<usize, Vec<String>> {
        let mut copied = 0;
        for path in paths {
            // A directory copied before may have brought a link with it.
            let done = match self.refusal(path) {
                Some(why) => Err(why),
                None if self.is_kept(path) => Ok(()),
                None => self
                    .copy_one(path)
                    .map_err(|why| format!("{} was not restored: {why}", Printable(path))),
            };
            match done {
                Ok(()) => copied += 1,
                Err(why) => return Err(vec![why, format!("{copied} restored before it")]),
            }
        }
        Ok(copied)
    }

    /// Whether `path` is a directory in the snapshot that the environment
    /// still has, which is left as it is.
    fn is_kept(&self, path: &str) -> bool {
        is_directory(self.files, &below(&self.from, path))
            && is_directory(self.files, &below(&self.into, path))
    }

    /// Copies `path` beside itself, then puts the copy in its place.
    fn copy_one(&self, path: &str) -> Result<(), String> {
        let source = below(&self.from, path);
        let target = below(&self.into, path);
        let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
            return Err("it has no name".to_owned());
        };
        let mut temporary = OsString::from(".");
        temporary.push(name);
        temporary.push(".zbm-restore");
        let temporary = parent.join(temporary);
        if is_there(self.files, &temporary) {
            return Err(format!("{} is in the way", shown(&temporary)));
        }
        let output = programs::copy_preserving(self.runner, &source, &temporary)
            .map_err(|error| format!("cp did not start: {error}"))?;
        if !output.status.is_success() {
            let why = String::from_utf8_lossy(&output.stderr);
            let why = match why.trim() {
                "" => "cp failed".to_owned(),
                why => Printable(why).to_string(),
            };
            return Err(self.discard(&temporary, why));
        }
        self.files.rename(&temporary, &target).map_err(|error| {
            self.discard(&temporary, format!("the copy did not replace it: {error}"))
        })
    }

    /// `why`, once the copy at `temporary` that did not replace its path is
    /// removed. A copied directory is left, as a restore removes no tree;
    /// `why` then names it, as it does a file that cannot be removed.
    fn discard(&self, temporary: &Path, why: String) -> String {
        if !is_there(self.files, temporary)
            || !is_directory(self.files, temporary) && self.files.remove_file(temporary).is_ok()
        {
            return why;
        }
        format!("{why}; its partial copy {} is left", shown(temporary))
    }
}

/// `path`, which holds a name from the environment, to show.
fn shown(path: &Path) -> String {
    Printable(&path.to_string_lossy()).to_string()
}

/// `path`, absolute in the environment, below `root`.
fn below(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

/// The directories above `path`, outermost first: `/a` and `/a/b` for
/// `/a/b/c`.
fn directories_above(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/')
        .skip(1)
        .map(|(at, _)| path.get(..at).unwrap_or_default())
}

/// Whether `path` is a symbolic link.
fn is_link(files: &dyn Files, path: &Path) -> bool {
    files.read_link(path).is_ok()
}

/// Whether there is anything at `path`, a dangling link included.
fn is_there(files: &dyn Files, path: &Path) -> bool {
    files.exists(path) || is_link(files, path)
}

/// Whether `path` is a directory, not a link to one.
fn is_directory(files: &dyn Files, path: &Path) -> bool {
    !is_link(files, path) && files.is_dir(path)
}

#[cfg(test)]
mod tests;
