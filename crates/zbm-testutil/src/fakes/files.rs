//! An in-memory file system.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use zbm_system::files::{FileKind, Files, Metadata};

/// A file system entry.
#[derive(Clone, Debug)]
enum Node {
    File {
        contents: Vec<u8>,
        kind: FileKind,
        mode: u32,
        modified: SystemTime,
    },
    Directory {
        mode: u32,
        modified: SystemTime,
    },
    Symlink(PathBuf),
}

/// A change the code under test made, in order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileOperation {
    /// A file was created or truncated and written.
    Write(PathBuf, Vec<u8>),
    /// Bytes were appended to a file.
    Append(PathBuf, Vec<u8>),
    /// A directory was created.
    CreateDir(PathBuf),
    /// A file or directory was removed.
    Remove(PathBuf),
    /// A file was renamed.
    Rename(PathBuf, PathBuf),
    /// Permission bits were set.
    SetMode(PathBuf, u32),
}

/// A component of a path, owned, so that link targets can be spliced in.
enum Part {
    Root,
    Parent,
    Name(OsString),
}

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

/// The modification time of the first change; each change is one second later.
const EPOCH: Duration = Duration::from_secs(1_700_000_000);

/// Symbolic links followed in one lookup before giving up, as Linux does.
const MAX_LINKS: usize = 40;

/// A [`Files`] that keeps everything in memory and records every change.
///
/// Paths must be absolute. Files, directories, device nodes and symbolic links
/// are set up with the builder methods, which do not count as changes; the
/// code under test then sees a file system with the usual errors (`NotFound`,
/// `AlreadyExists`, `NotADirectory` and so on). Modification times advance by
/// one second with each change, so "newer than" comparisons are predictable.
///
/// Clones share one file system, so a stand-in for a program (a responder of
/// the fake runner) can change the files the code under test sees.
#[derive(Clone)]
pub struct FakeFiles {
    nodes: Rc<RefCell<BTreeMap<PathBuf, Node>>>,
    operations: Rc<RefCell<Vec<FileOperation>>>,
    failures: Rc<RefCell<Vec<(PathBuf, io::ErrorKind)>>>,
    ticks: Rc<Cell<u64>>,
}

impl Default for FakeFiles {
    fn default() -> FakeFiles {
        FakeFiles::new()
    }
}

/// The error the kernel reports for `kind`, so that messages read as they do on
/// a real system ("No such file or directory").
fn error(kind: io::ErrorKind, path: &Path) -> io::Error {
    let errno = match kind {
        io::ErrorKind::NotFound => 2,
        io::ErrorKind::PermissionDenied => 13,
        io::ErrorKind::AlreadyExists => 17,
        io::ErrorKind::NotADirectory => 20,
        io::ErrorKind::IsADirectory => 21,
        io::ErrorKind::InvalidInput => 22,
        io::ErrorKind::StorageFull => 28,
        io::ErrorKind::ReadOnlyFilesystem => 30,
        io::ErrorKind::DirectoryNotEmpty => 39,
        // ELOOP, for too many symbolic links.
        io::ErrorKind::Other => 40,
        _ => return io::Error::new(kind, format!("{}: {kind}", path.display())),
    };
    io::Error::from_raw_os_error(errno)
}

impl FakeFiles {
    /// A file system with only the root directory.
    pub fn new() -> FakeFiles {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            PathBuf::from("/"),
            Node::Directory {
                mode: 0o755,
                modified: SystemTime::UNIX_EPOCH + EPOCH,
            },
        );
        FakeFiles {
            nodes: Rc::new(RefCell::new(nodes)),
            operations: Rc::new(RefCell::new(Vec::new())),
            failures: Rc::new(RefCell::new(Vec::new())),
            ticks: Rc::new(Cell::new(0)),
        }
    }

    fn tick(&self) -> SystemTime {
        let ticks = self.ticks.get() + 1;
        self.ticks.set(ticks);
        SystemTime::UNIX_EPOCH + EPOCH + Duration::from_secs(ticks)
    }

    /// Adds directories along `path`, for the builder methods.
    fn make_parents(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            self.dir(parent);
        }
    }

    /// Adds a directory and its parents.
    pub fn dir(&self, path: impl AsRef<Path>) -> &FakeFiles {
        let path = path.as_ref();
        let mut current = PathBuf::from("/");
        for component in path.components().skip(1) {
            current.push(component);
            self.nodes
                .borrow_mut()
                .entry(current.clone())
                .or_insert(Node::Directory {
                    mode: 0o755,
                    modified: SystemTime::UNIX_EPOCH + EPOCH,
                });
        }
        self
    }

    /// Adds a regular file with mode 0644, and its parent directories.
    pub fn file(&self, path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> &FakeFiles {
        self.file_with_mode(path, contents, 0o644)
    }

    /// Adds a regular file with the given mode, and its parent directories.
    pub fn file_with_mode(
        &self,
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
        mode: u32,
    ) -> &FakeFiles {
        let path = path.as_ref();
        self.make_parents(path);
        self.nodes.borrow_mut().insert(
            path.to_owned(),
            Node::File {
                contents: contents.as_ref().to_vec(),
                kind: FileKind::File,
                mode,
                modified: SystemTime::UNIX_EPOCH + EPOCH,
            },
        );
        self
    }

    /// Adds a device node (or another special file), such as `/dev/kmsg` or a
    /// link target in `/dev/disk/by-id`. Writes to it are recorded and kept.
    pub fn device(&self, path: impl AsRef<Path>) -> &FakeFiles {
        self.special(path.as_ref(), FileKind::Other)
    }

    /// Adds a block device node, such as `/dev/sda`.
    pub fn block_device(&self, path: impl AsRef<Path>) -> &FakeFiles {
        self.special(path.as_ref(), FileKind::BlockDevice)
    }

    fn special(&self, path: &Path, kind: FileKind) -> &FakeFiles {
        self.make_parents(path);
        self.nodes.borrow_mut().insert(
            path.to_owned(),
            Node::File {
                contents: Vec::new(),
                kind,
                mode: 0o600,
                modified: SystemTime::UNIX_EPOCH + EPOCH,
            },
        );
        self
    }

    /// Adds a symbolic link to `target`, which may be relative to the link's
    /// directory.
    pub fn symlink(&self, path: impl AsRef<Path>, target: impl AsRef<Path>) -> &FakeFiles {
        let path = path.as_ref();
        self.make_parents(path);
        self.nodes
            .borrow_mut()
            .insert(path.to_owned(), Node::Symlink(target.as_ref().to_owned()));
        self
    }

    /// Makes every later operation on `path` fail with `kind`.
    pub fn fail(&self, path: impl AsRef<Path>, kind: io::ErrorKind) -> &FakeFiles {
        self.failures
            .borrow_mut()
            .push((path.as_ref().to_owned(), kind));
        self
    }

    /// Sets the modification time of an entry.
    pub fn set_modified(&self, path: impl AsRef<Path>, time: SystemTime) {
        let path = path.as_ref();
        match self.nodes.borrow_mut().get_mut(path) {
            Some(Node::File { modified, .. } | Node::Directory { modified, .. }) => {
                *modified = time;
            }
            _ => panic!("{} is not a file or directory", path.display()),
        }
    }

    /// The contents of a file, following links, if it exists.
    pub fn contents(&self, path: impl AsRef<Path>) -> Option<Vec<u8>> {
        let path = self.resolve(path.as_ref(), true).ok()?;
        match self.nodes.borrow().get(&path)? {
            Node::File { contents, .. } => Some(contents.clone()),
            _ => None,
        }
    }

    /// The contents of a file as text, if it exists.
    pub fn text(&self, path: impl AsRef<Path>) -> Option<String> {
        self.contents(path)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    /// The changes made so far, in order.
    pub fn operations(&self) -> Vec<FileOperation> {
        self.operations.borrow().clone()
    }

    /// Every path that exists, sorted.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.nodes.borrow().keys().cloned().collect()
    }

    fn record(&self, operation: FileOperation) {
        self.operations.borrow_mut().push(operation);
    }

    fn check_failure(&self, path: &Path) -> io::Result<()> {
        match self
            .failures
            .borrow()
            .iter()
            .find(|(failing, _)| failing == path)
        {
            Some((_, kind)) => Err(error(*kind, path)),
            None => Ok(()),
        }
    }

    /// The path without symbolic links (the last one only if `follow_last`),
    /// `.` or `..`. Every directory on the way must exist; the last component
    /// need not.
    fn resolve(&self, path: &Path, follow_last: bool) -> io::Result<PathBuf> {
        if !path.is_absolute() {
            return Err(error(io::ErrorKind::InvalidInput, path));
        }
        let nodes = self.nodes.borrow();
        let mut pending: VecDeque<Part> = parts(path).into();
        let mut resolved = PathBuf::from("/");
        let mut links = 0;
        while let Some(part) = pending.pop_front() {
            let name = match part {
                Part::Root => {
                    resolved = PathBuf::from("/");
                    continue;
                }
                Part::Parent => {
                    resolved.pop();
                    continue;
                }
                Part::Name(name) => name,
            };
            let candidate = resolved.join(&name);
            let last = pending.is_empty();
            match nodes.get(&candidate) {
                Some(Node::Symlink(target)) if follow_last || !last => {
                    links += 1;
                    if links > MAX_LINKS {
                        return Err(error(io::ErrorKind::Other, path));
                    }
                    // A relative target starts from the link's directory.
                    for part in parts(&resolved.join(target)).into_iter().rev() {
                        pending.push_front(part);
                    }
                }
                Some(Node::Directory { .. }) => resolved = candidate,
                Some(_) | None if last => resolved = candidate,
                Some(_) => return Err(error(io::ErrorKind::NotADirectory, path)),
                None => return Err(error(io::ErrorKind::NotFound, path)),
            }
        }
        Ok(resolved)
    }

    /// Resolves `path` for creating or changing it: its parent must be an
    /// existing directory.
    fn resolve_for_write(&self, path: &Path, follow_last: bool) -> io::Result<PathBuf> {
        self.check_failure(path)?;
        let resolved = self.resolve(path, follow_last)?;
        let parent = resolved.parent().unwrap_or(Path::new("/"));
        match self.nodes.borrow().get(parent) {
            Some(Node::Directory { .. }) => Ok(resolved),
            Some(_) => Err(error(io::ErrorKind::NotADirectory, path)),
            None => Err(error(io::ErrorKind::NotFound, path)),
        }
    }

    fn write_file(&self, path: &Path, contents: &[u8], append: bool) -> io::Result<()> {
        let resolved = self.resolve_for_write(path, true)?;
        let modified = self.tick();
        let mut nodes = self.nodes.borrow_mut();
        match nodes.get_mut(&resolved) {
            Some(Node::Directory { .. }) => return Err(error(io::ErrorKind::IsADirectory, path)),
            Some(Node::File {
                contents: existing,
                modified: time,
                ..
            }) => {
                if !append {
                    existing.clear();
                }
                existing.extend_from_slice(contents);
                *time = modified;
            }
            Some(Node::Symlink(_)) => unreachable!("links are resolved"),
            None => {
                nodes.insert(
                    resolved.clone(),
                    Node::File {
                        contents: contents.to_vec(),
                        kind: FileKind::File,
                        mode: 0o644,
                        modified,
                    },
                );
            }
        }
        drop(nodes);
        self.record(if append {
            FileOperation::Append(resolved, contents.to_vec())
        } else {
            FileOperation::Write(resolved, contents.to_vec())
        });
        Ok(())
    }
}

impl Files for FakeFiles {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.check_failure(path)?;
        let resolved = self.resolve(path, true)?;
        match self.nodes.borrow().get(&resolved) {
            Some(Node::File { contents, .. }) => Ok(contents.clone()),
            Some(Node::Directory { .. }) => Err(error(io::ErrorKind::IsADirectory, path)),
            Some(Node::Symlink(_)) => unreachable!("links are resolved"),
            None => Err(error(io::ErrorKind::NotFound, path)),
        }
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        self.write_file(path, contents, false)
    }

    fn append(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        self.write_file(path, contents, true)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        let resolved = self.resolve_for_write(path, false)?;
        if self.nodes.borrow().contains_key(&resolved) {
            return Err(error(io::ErrorKind::AlreadyExists, path));
        }
        let modified = self.tick();
        self.nodes.borrow_mut().insert(
            resolved.clone(),
            Node::Directory {
                mode: 0o755,
                modified,
            },
        );
        self.record(FileOperation::CreateDir(resolved));
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.check_failure(path)?;
        let mut current = PathBuf::from("/");
        for component in path.components().skip(1) {
            current.push(component);
            match self.metadata(&current) {
                Ok(metadata) if metadata.kind == FileKind::Directory => {}
                Ok(_) => return Err(error(io::ErrorKind::AlreadyExists, &current)),
                Err(_) => self.create_dir(&current)?,
            }
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let resolved = self.resolve_for_write(path, false)?;
        let mut nodes = self.nodes.borrow_mut();
        match nodes.get(&resolved) {
            Some(Node::Directory { .. }) => Err(error(io::ErrorKind::IsADirectory, path)),
            Some(_) => {
                nodes.remove(&resolved);
                drop(nodes);
                self.record(FileOperation::Remove(resolved));
                Ok(())
            }
            None => Err(error(io::ErrorKind::NotFound, path)),
        }
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        let resolved = self.resolve_for_write(path, false)?;
        let mut nodes = self.nodes.borrow_mut();
        match nodes.get(&resolved) {
            Some(Node::Directory { .. }) => {
                let has_children = nodes
                    .keys()
                    .any(|key| key.parent() == Some(resolved.as_path()));
                if has_children {
                    return Err(error(io::ErrorKind::DirectoryNotEmpty, path));
                }
                nodes.remove(&resolved);
                drop(nodes);
                self.record(FileOperation::Remove(resolved));
                Ok(())
            }
            Some(_) => Err(error(io::ErrorKind::NotADirectory, path)),
            None => Err(error(io::ErrorKind::NotFound, path)),
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let source = self.resolve_for_write(from, false)?;
        let target = self.resolve_for_write(to, false)?;
        let mut nodes = self.nodes.borrow_mut();
        if !nodes.contains_key(&source) {
            return Err(error(io::ErrorKind::NotFound, from));
        }
        if matches!(nodes.get(&target), Some(Node::Directory { .. })) {
            return Err(error(io::ErrorKind::IsADirectory, to));
        }
        let moved: Vec<PathBuf> = nodes
            .keys()
            .filter(|key| key.starts_with(&source))
            .cloned()
            .collect();
        for old in moved {
            let node = nodes.remove(&old).expect("the key was listed");
            let suffix = old
                .strip_prefix(&source)
                .expect("the key is below the source");
            let new = if suffix.as_os_str().is_empty() {
                target.clone()
            } else {
                target.join(suffix)
            };
            nodes.insert(new, node);
        }
        drop(nodes);
        self.record(FileOperation::Rename(source, target));
        Ok(())
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<OsString>> {
        self.check_failure(path)?;
        let resolved = self.resolve(path, true)?;
        let nodes = self.nodes.borrow();
        match nodes.get(&resolved) {
            Some(Node::Directory { .. }) => {}
            Some(_) => return Err(error(io::ErrorKind::NotADirectory, path)),
            None => return Err(error(io::ErrorKind::NotFound, path)),
        }
        let mut names: Vec<OsString> = nodes
            .keys()
            .filter(|key| key.parent() == Some(resolved.as_path()))
            .filter_map(|key| key.file_name().map(ToOwned::to_owned))
            .collect();
        names.sort_by(|a, b| a.as_encoded_bytes().cmp(b.as_encoded_bytes()));
        Ok(names)
    }

    fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        self.check_failure(path)?;
        let resolved = self.resolve(path, true)?;
        match self.nodes.borrow().get(&resolved) {
            Some(Node::File {
                contents,
                kind,
                mode,
                modified,
            }) => Ok(Metadata {
                kind: *kind,
                mode: *mode,
                len: contents.len() as u64,
                modified: *modified,
            }),
            Some(Node::Directory { mode, modified }) => Ok(Metadata {
                kind: FileKind::Directory,
                mode: *mode,
                len: 0,
                modified: *modified,
            }),
            Some(Node::Symlink(_)) => unreachable!("links are resolved"),
            None => Err(error(io::ErrorKind::NotFound, path)),
        }
    }

    fn set_mode(&self, path: &Path, new_mode: u32) -> io::Result<()> {
        self.check_failure(path)?;
        let resolved = self.resolve(path, true)?;
        match self.nodes.borrow_mut().get_mut(&resolved) {
            Some(Node::File { mode, .. } | Node::Directory { mode, .. }) => {
                *mode = new_mode & 0o7777
            }
            _ => return Err(error(io::ErrorKind::NotFound, path)),
        }
        self.record(FileOperation::SetMode(resolved, new_mode & 0o7777));
        Ok(())
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.check_failure(path)?;
        let resolved = self.resolve(path, true)?;
        if self.nodes.borrow().contains_key(&resolved) {
            Ok(resolved)
        } else {
            Err(error(io::ErrorKind::NotFound, path))
        }
    }
}
