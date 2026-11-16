//! The files of a boot environment as the kernel search opens them:
//! resolved inside the environment's root, as its own programs would find
//! them, never in ZFSBootMenu's namespace; with a UTF-8 path that has no
//! link on it; and only regular files of bounded size.
//!
//! An absolute link starts again at the environment's root and `..` stops
//! there, so no link leads out of it; a path into the environment's
//! `/tmp`, `/proc`, `/sys`, `/dev` or `.zfs` is refused (threat model I5).

use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

use zbm_platform::files::{FileKind, Files};
use zbm_tools::mount::inside::{self, Unresolved};
use zbm_types::Printable;

/// The most bytes of a command line file (`.kcl`) or an `os-release` that
/// are read: only a first line is used, and a kernel's command line is a
/// few KiB at most.
pub(crate) const SMALL_LIMIT: u64 = 64 << 10;

/// The largest kernel, initramfs or device tree that is read: far above
/// any real image.
const IMAGE_LIMIT: u64 = 1 << 30;

/// The most kernels of one `/boot` that are kept, the newest.
pub(crate) const MAX_KERNELS: usize = 64;

/// The longest name a directory can hold (`NAME_MAX`): a longer one cannot
/// exist, so it is not looked up.
pub(crate) const NAME_MAX: usize = 255;

/// Why a file of a boot environment is not read. The paths are the
/// environment's, as it names them from its root.
#[derive(Debug)]
pub enum Refused {
    /// The path leads nowhere inside the environment: a missing name, a
    /// name that is no directory where names follow, the environment's
    /// `.zfs`, too many links or names, or a name that could not be read.
    Unresolved(Unresolved),
    /// The path leads into the environment's `/tmp`, `/proc`, `/sys` or
    /// `/dev`.
    Covered(PathBuf),
    /// The file is no regular file: a directory, pipe, device or socket.
    NotAFile(PathBuf, FileKind),
    /// `/boot`, its links followed, is no directory: a file, pipe, device
    /// or socket.
    NotADirectory(PathBuf, FileKind),
    /// The file is larger, in bytes, than what is read.
    TooLarge(PathBuf, u64),
    /// The path, its links followed, is not UTF-8, so it could not be
    /// handed on as it is.
    NotUtf8(PathBuf),
}

impl Refused {
    /// Whether the file is simply not there: a missing name, a name that
    /// is no directory, or a name too long to exist.
    pub(crate) fn is_absent(&self) -> bool {
        match self {
            Refused::Unresolved(Unresolved::Missing { .. } | Unresolved::NotADirectory(_)) => true,
            Refused::Unresolved(Unresolved::Io(error)) => matches!(
                error.kind(),
                io::ErrorKind::InvalidFilename | io::ErrorKind::NotFound
            ),
            _ => false,
        }
    }
}

/// What a file that is no regular file is, in plain words.
fn kind_words(kind: FileKind) -> &'static str {
    match kind {
        FileKind::File => "a file",
        FileKind::Directory => "a directory",
        FileKind::BlockDevice => "a block device",
        FileKind::CharDevice => "a character device",
        FileKind::Socket => "a socket",
        FileKind::Other => "a pipe or other special file",
    }
}

/// Why, in plain words. The paths in it are the environment's, shown as
/// they are, so a caller that shows them escapes them.
impl fmt::Display for Refused {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::Unresolved(unresolved) => write!(formatter, "{unresolved}"),
            Refused::Covered(path) => {
                let top = path
                    .components()
                    .find(|component| matches!(component, Component::Normal(_)))
                    .map_or(Path::new(""), |top| Path::new(top.as_os_str()));
                write!(
                    formatter,
                    "it leads into the environment's /{}, which is not read",
                    top.display()
                )
            }
            Refused::NotAFile(path, kind) => write!(
                formatter,
                "{} is {}, not a regular file",
                path.display(),
                kind_words(*kind)
            ),
            Refused::NotADirectory(path, kind) => write!(
                formatter,
                "{} is {}, not a directory",
                path.display(),
                kind_words(*kind)
            ),
            Refused::TooLarge(path, len) => write!(
                formatter,
                "{} has {len} bytes, more than is read",
                path.display()
            ),
            Refused::NotUtf8(path) => {
                write!(formatter, "{} is not a UTF-8 path", path.display())
            }
        }
    }
}

/// The kernel, initramfs or device tree `inside` (absolute, as the
/// environment names it) of the boot environment mounted at `mnt`: its
/// path with no link on it, below `mnt`, as text, when it is a regular file
/// of at most 1 GiB.
///
/// # Errors
///
/// [`Refused`]: why the file is not read.
pub fn boot_image(files: &dyn Files, mnt: &Path, inside: &Path) -> Result<String, Refused> {
    boot_file(files, mnt, inside, IMAGE_LIMIT)
}

/// The command line file or `os-release` `inside` of the boot environment
/// mounted at `mnt`, as [`boot_image`] finds a kernel, of at most
/// [`SMALL_LIMIT`] bytes.
pub(crate) fn small_file(files: &dyn Files, mnt: &Path, inside: &Path) -> Result<String, Refused> {
    boot_file(files, mnt, inside, SMALL_LIMIT)
}

/// The file `inside` of the boot environment mounted at `mnt`, resolved
/// inside its root, when it is a regular file of at most `limit` bytes
/// with a UTF-8 path: that path, with no link on it, below `mnt`.
fn boot_file(files: &dyn Files, mnt: &Path, inside: &Path, limit: u64) -> Result<String, Refused> {
    let found = inside::resolve(files, mnt, inside).map_err(refused)?;
    let named = found.path.in_chroot();
    let path = found.path.under(mnt);
    let Some(text) = path.to_str() else {
        return Err(Refused::NotUtf8(named));
    };
    let metadata = files
        .metadata(&path)
        .map_err(|error| Refused::Unresolved(Unresolved::Io(error)))?;
    if metadata.kind != FileKind::File {
        return Err(Refused::NotAFile(named, metadata.kind));
    }
    if metadata.len > limit {
        return Err(Refused::TooLarge(named, metadata.len));
    }
    Ok(text.to_owned())
}

/// Why a path that leads nowhere inside the environment is not read, in
/// the boot's words: a path into its `/tmp`, `/proc`, `/sys` or `/dev` is
/// the environment's, which the boot does not cover with this system's.
pub(crate) fn refused(unresolved: Unresolved) -> Refused {
    match unresolved {
        Unresolved::Covered(path) => Refused::Covered(path.in_chroot()),
        other => Refused::Unresolved(other),
    }
}

/// The files of one kernel that exist and are refused, which make one
/// warning for the kernel; each one's reason goes to the debug lines. All
/// of it is escaped.
pub(crate) struct Refusals<'a> {
    /// The boot environment.
    dataset: &'a str,
    /// The kernel, as the environment names it.
    kernel: &'a str,
    /// How many files were refused.
    count: usize,
    /// Why the first one was.
    first: Option<String>,
}

impl<'a> Refusals<'a> {
    /// No file of `kernel` of `dataset` refused yet.
    pub(crate) fn new(dataset: &'a str, kernel: &'a str) -> Refusals<'a> {
        Refusals {
            dataset,
            kernel,
            count: 0,
            first: None,
        }
    }

    /// Notes why the file `name` is not read: nothing for a file that is
    /// not there, a debug line alone for a directory, and a debug line for
    /// any other, which counts for the warning.
    pub(crate) fn note(&mut self, name: &str, why: &Refused) {
        if why.is_absent() {
            return;
        }
        let reason = why.to_string();
        zbm_trace::debug!(
            "{file} of {dataset} is not read: {why}";
            file = %Printable(name), dataset = %Printable(self.dataset), why = %Printable(&reason)
        );
        if matches!(why, Refused::NotAFile(_, FileKind::Directory)) {
            return;
        }
        self.count += 1;
        self.first
            .get_or_insert_with(|| format!("{name}: {reason}"));
    }

    /// Warns once for the kernel, when any of its files was refused.
    pub(crate) fn report(self) {
        if let Some(first) = &self.first {
            zbm_trace::warn!(
                "kernel {kernel} of {dataset}: {count} of its files not read, the first: {why}";
                kernel = %Printable(self.kernel),
                dataset = %Printable(self.dataset),
                count = self.count,
                why = %Printable(first)
            );
        }
    }
}
