//! Where a path leads in a chroot: its symbolic links followed inside the
//! chroot's root, as the chroot's programs will follow them, never in this
//! system's namespace, where an absolute link would lead out of the root
//! ([`resolve`]); and the first link on a path below the root, with nothing
//! followed ([`InRoot::first_link`]). Nothing here runs a program.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

use zbm_platform::Files;

/// The symbolic links followed in one walk before it gives up, as Linux
/// does.
const MAX_LINKS: usize = 40;

/// The directories that the chroot covers with this system's: what is
/// below them in the root is hidden there.
const COVERED: [&str; 4] = ["tmp", "proc", "sys", "dev"];

/// A path inside a chroot's root, held as its names below the root: no
/// root directory, no `.` and no `..`, so that it never replaces the root
/// it is put below.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InRoot(Vec<OsString>);

impl InRoot {
    /// `path`'s names, a root directory and `.` dropped; `None` when it has
    /// `..`.
    pub fn new(path: &Path) -> Option<InRoot> {
        path.components()
            .filter_map(|component| match component {
                Component::Normal(name) => Some(Some(name.to_owned())),
                Component::ParentDir => Some(None),
                Component::RootDir | Component::CurDir | Component::Prefix(_) => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(InRoot)
    }

    /// The path below `root`, where this system finds it: `root` with each
    /// name added, never a path that replaces `root`.
    pub fn under(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        for name in &self.0 {
            path.push(name);
        }
        path
    }

    /// The path as the chroot names it, from its root directory.
    pub fn in_chroot(&self) -> PathBuf {
        self.under(Path::new("/"))
    }

    /// The directory of the root the path is in: its first name alone.
    #[must_use]
    pub fn top(&self) -> InRoot {
        InRoot(self.0.first().cloned().into_iter().collect())
    }

    /// Whether the path is below the directory `name` of the root, `name`
    /// itself not counted.
    pub fn is_below(&self, name: &str) -> bool {
        self.0.len() > 1 && self.0.first().is_some_and(|first| first == name)
    }

    /// The first name of the path, below `root`, that is a symbolic link,
    /// with its target, read shortest first, nothing followed. `None` when
    /// there is none, and also when a name is missing (its read's `ENOENT`)
    /// or is no directory where names follow (the next name's `ENOTDIR`):
    /// nothing below it can be followed then, and a mount or an unmount of
    /// the path fails by itself. The last name may be a file.
    ///
    /// Each name read is below a path found to be no link, so no read
    /// looks at a path that a link of the root leads to.
    ///
    /// # Errors
    ///
    /// A link that could not be read, other than for those two reasons.
    pub fn first_link(
        &self,
        files: &dyn Files,
        root: &Path,
    ) -> io::Result<Option<(InRoot, PathBuf)>> {
        let mut walked = InRoot::default();
        for name in &self.0 {
            walked.0.push(name.clone());
            match files.read_link(&walked.under(root)) {
                Ok(target) => return Ok(Some((walked, target))),
                Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    /// Whether the chroot covers the path with this system's files: it is
    /// one of the [`COVERED`] directories, or below one. Names are compared
    /// whole, so `/devices` is not covered.
    fn is_covered(&self) -> bool {
        self.0
            .first()
            .is_some_and(|first| COVERED.iter().any(|covered| first == covered))
    }

    /// The path with `name` added.
    fn with(&self, name: OsString) -> InRoot {
        let mut names = self.0.clone();
        names.push(name);
        InRoot(names)
    }
}

/// Where a path leads in the chroot whose root is mounted at `root`.
#[derive(Debug, Eq, PartialEq)]
pub struct Inside {
    /// The path with no link on the way, the last name included.
    pub path: InRoot,
    /// Whether a link was followed to get there.
    pub followed: bool,
}

/// Why a path leads nowhere [`resolve`] can name.
#[derive(Debug)]
pub enum Unresolved {
    /// A name on the way does not exist: the path as far as it exists,
    /// with the missing name and, when only names follow it, those names.
    Missing {
        /// What the path would be.
        path: InRoot,
        /// Whether only names follow the missing one, no `..`, so that
        /// `path` is where the path leads once they are made.
        names_only: bool,
    },
    /// The path reaches `/tmp`, `/proc`, `/sys` or `/dev`, which the
    /// chroot covers with this system's.
    Covered(InRoot),
    /// A name on the way is no directory.
    NotADirectory(InRoot),
    /// More than 40 links on the way, as Linux allows.
    TooManyLinks,
    /// A link could not be read.
    Io(io::Error),
}

/// Why, in plain words. The names in it are the environment's, shown as
/// they are, so a caller that shows them escapes them.
impl fmt::Display for Unresolved {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unresolved::Missing { path, .. } => {
                let path = path.in_chroot();
                write!(
                    formatter,
                    "{} is missing in the environment",
                    path.display()
                )
            }
            Unresolved::Covered(path) => {
                let top = path.top().in_chroot();
                write!(formatter, "it leads into this system's {}", top.display())
            }
            Unresolved::NotADirectory(path) => {
                let path = path.in_chroot();
                write!(formatter, "{} is not a directory", path.display())
            }
            Unresolved::TooManyLinks => formatter.write_str("too many symbolic links"),
            Unresolved::Io(error) => write!(formatter, "{error}"),
        }
    }
}

/// A component of a path, owned, so that a link's target can go before
/// the names still to look up.
enum Part {
    Root,
    Parent,
    Name(OsString),
}

/// The parts of `path`; `.` is left out.
fn parts(path: &Path) -> Vec<Part> {
    path.components()
        .filter_map(|component| match component {
            Component::RootDir => Some(Part::Root),
            Component::ParentDir => Some(Part::Parent),
            Component::Normal(name) => Some(Part::Name(name.to_owned())),
            Component::CurDir | Component::Prefix(_) => None,
        })
        .collect()
}

/// Where `path` leads in the chroot whose root is mounted at `root`, with
/// `files`: each name looked up below the root, a link read there and its
/// target followed from the root (an absolute one) or from the link's
/// directory (a relative one), and `..` never above the root. Only a name
/// of a path with no link is ever read, so nothing outside the root is
/// looked up.
///
/// # Errors
///
/// [`Unresolved`]: a name that is missing, covered by this system's, or no
/// directory where names follow; too many links; or a link that could not
/// be read.
pub fn resolve(files: &dyn Files, root: &Path, path: &Path) -> Result<Inside, Unresolved> {
    let mut pending: VecDeque<Part> = parts(path).into();
    let mut resolved = InRoot::default();
    let (mut links, mut followed) = (0, false);
    while let Some(part) = pending.pop_front() {
        let name = match part {
            Part::Root => {
                resolved = InRoot::default();
                continue;
            }
            Part::Parent => {
                resolved.0.pop();
                continue;
            }
            Part::Name(name) => name,
        };
        let candidate = resolved.with(name);
        if candidate.is_covered() {
            return Err(Unresolved::Covered(candidate));
        }
        match files.read_link(&candidate.under(root)) {
            Ok(target) => {
                links += 1;
                if links > MAX_LINKS {
                    return Err(Unresolved::TooManyLinks);
                }
                followed = true;
                for part in parts(&target).into_iter().rev() {
                    pending.push_front(part);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
                if !pending.is_empty() && !files.is_dir(&candidate.under(root)) {
                    return Err(Unresolved::NotADirectory(candidate));
                }
                resolved = candidate;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(missing(candidate, pending));
            }
            Err(error) => return Err(Unresolved::Io(error)),
        }
    }
    Ok(Inside {
        path: resolved,
        followed,
    })
}

/// [`Unresolved::Missing`] for the missing `candidate`, with the `pending`
/// names after it when only names follow.
fn missing(candidate: InRoot, pending: VecDeque<Part>) -> Unresolved {
    let names_only = pending.iter().all(|part| matches!(part, Part::Name(_)));
    let mut path = candidate;
    if names_only {
        for part in pending {
            if let Part::Name(name) = part {
                path.0.push(name);
            }
        }
    }
    Unresolved::Missing { path, names_only }
}

#[cfg(test)]
mod tests;
