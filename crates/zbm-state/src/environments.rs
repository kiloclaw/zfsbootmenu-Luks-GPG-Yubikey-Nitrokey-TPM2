//! The state of each filesystem: its directory below `environments`, with
//! its mount point (`mnt`), its kernel list (`kernels`) and its command
//! line cache (`kcl`), beside the directories of its children and of its
//! snapshots.
//!
//! The directory is the legacy runtime's, `environments/<name>`, but for
//! one escape: a piece of the name between two `/` that is `mnt`,
//! `kernels` or `kcl` gets a `+` before it, so `pool/a/mnt` has
//! `environments/pool/a/+mnt`. Without it, the state of `pool/a/mnt` would
//! be `pool/a`'s mount point, and its mount and files would land in
//! `pool/a`'s root, through its links, while `pool/a` is mounted. A piece
//! that is empty, `.` or `..`, or that starts with `+`, which no dataset
//! name has, is escaped too, so that any text, a dataset name or not, has
//! a directory of its own below `environments`, and no two texts share
//! one or have one inside another's mount point, kernel list or cache.

use std::path::PathBuf;

use zbm_platform::error::{Error, Result};

use super::State;

/// Where a filesystem is mounted, in its directory.
const MOUNT_POINT: &str = "mnt";
/// The kernel list of a filesystem, in its directory.
const KERNEL_LIST: &str = "kernels";
/// The command line cache of a filesystem, in its directory.
const KCL_CACHE: &str = "kcl";

/// The entries of a filesystem's directory that are not the directory of
/// a child: a piece of a name that is one of them is escaped.
const ENVIRONMENT_ENTRIES: [&str; 3] = [MOUNT_POINT, KERNEL_LIST, KCL_CACHE];

/// Whether `piece`, a piece of a name between two `/`, is written with a
/// `+` before it.
fn is_escaped(piece: &str) -> bool {
    piece.is_empty()
        || piece == "."
        || piece == ".."
        || piece.starts_with('+')
        || ENVIRONMENT_ENTRIES.contains(&piece)
}

/// `name` as its directory below `environments`: split on each `/`, each
/// piece [`is_escaped`] with a `+` before it, joined again. The result has
/// no empty, `.` or `..` piece, so it is always relative and stays below
/// `environments`.
fn escaped(name: &str) -> String {
    let pieces: Vec<String> = name
        .split('/')
        .map(|piece| {
            if is_escaped(piece) {
                format!("+{piece}")
            } else {
                piece.to_owned()
            }
        })
        .collect();
    pieces.join("/")
}

impl State<'_> {
    /// The directory of the state of every filesystem (`environments`).
    pub fn environments(&self) -> PathBuf {
        self.path("environments")
    }

    /// The directory of the state of the filesystem `dataset`, as the
    /// module says.
    fn directory(&self, dataset: &str) -> PathBuf {
        self.environments().join(escaped(dataset))
    }

    /// The directory of a boot environment's state,
    /// `environments/<dataset>`, created if needed.
    ///
    /// # Errors
    ///
    /// When the directory cannot be created.
    pub fn environment(&self, dataset: &str) -> Result<PathBuf> {
        let path = self.directory(dataset);
        self.files
            .create_dir_all(&path)
            .map_err(|error| Error::io(format!("unable to create {}", path.display()), error))?;
        Ok(path)
    }

    /// Where a boot environment is mounted (`environments/<dataset>/mnt`).
    pub fn mount_point(&self, dataset: &str) -> PathBuf {
        self.directory(dataset).join(MOUNT_POINT)
    }

    /// The kernel list of a boot environment (`environments/<dataset>/kernels`).
    pub fn kernels(&self, dataset: &str) -> PathBuf {
        self.directory(dataset).join(KERNEL_LIST)
    }

    /// The cached command line of a kernel of a boot environment, named after
    /// the kernel: the kernel's path without its leading `/`,
    /// with `:` doubled and `/` replaced by `:`, in
    /// `environments/<dataset>/kcl/<name>.kcl`. `None` without a kernel.
    pub fn kcl_cache(&self, dataset: &str, kernel: &str) -> Option<PathBuf> {
        if kernel.is_empty() {
            return None;
        }
        let flat = kernel
            .strip_prefix('/')
            .unwrap_or(kernel)
            .replace(':', "::")
            .replace('/', ":");
        Some(
            self.directory(dataset)
                .join(KCL_CACHE)
                .join(format!("{flat}.kcl")),
        )
    }
}

#[cfg(test)]
mod tests;
