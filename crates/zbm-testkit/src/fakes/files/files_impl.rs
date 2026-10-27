//! The [`Files`] interface of [`FakeFiles`].

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use zbm_platform::files::{FileKind, FileLock, Files, LockWait, Metadata};

use super::{FakeFiles, FileOperation, Node, error, lock};

/// A fake lock: the entry of the table, removed when the lock is dropped.
struct Held {
    locks: Arc<std::sync::Mutex<std::collections::BTreeSet<PathBuf>>>,
    path: PathBuf,
}

impl Drop for Held {
    fn drop(&mut self) {
        lock(&self.locks).remove(&self.path);
    }
}

impl Files for FakeFiles {
    fn shared(&self) -> Arc<dyn Files + Send + Sync> {
        Arc::new(self.clone())
    }

    fn read_prefix(&self, path: &Path, len: usize) -> io::Result<Vec<u8>> {
        let mut data = self.read(path)?;
        data.truncate(len);
        Ok(data)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.check_failure(path)?;
        let resolved = self.resolve(path, true)?;
        match self.nodes().get(&resolved) {
            Some(Node::File { contents, .. }) => Ok(contents.clone()),
            Some(Node::Directory { .. }) => Err(error(io::ErrorKind::IsADirectory, path)),
            Some(Node::Symlink(_)) => unreachable!("links are resolved"),
            None => Err(error(io::ErrorKind::NotFound, path)),
        }
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        self.write_file(path, contents, false)
    }

    fn write_private(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        // The last component is not followed: a link there exists.
        let resolved = self.resolve_for_write(path, false)?;
        if self.nodes().contains_key(&resolved) {
            return Err(error(io::ErrorKind::AlreadyExists, path));
        }
        let modified = self.tick();
        self.nodes().insert(
            resolved.clone(),
            Node::File {
                contents: contents.to_vec(),
                kind: FileKind::File,
                mode: 0o600,
                modified,
            },
        );
        self.record(FileOperation::Write(resolved, contents.to_vec()));
        Ok(())
    }

    fn lock(&self, path: &Path, wait: LockWait) -> io::Result<Option<FileLock>> {
        let resolved = self.resolve_for_write(path, true)?;
        if !self.nodes().contains_key(&resolved) {
            let modified = self.tick();
            self.nodes().insert(
                resolved.clone(),
                Node::File {
                    contents: Vec::new(),
                    kind: FileKind::File,
                    mode: 0o600,
                    modified,
                },
            );
        }
        if !lock(&self.locks).insert(resolved.clone()) {
            return match wait {
                LockWait::Try => Ok(None),
                LockWait::Block => Err(error(io::ErrorKind::WouldBlock, path)),
            };
        }
        Ok(Some(FileLock::new(Held {
            locks: Arc::clone(&self.locks),
            path: resolved,
        })))
    }

    fn is_locked(&self, path: &Path) -> bool {
        self.resolve_for_write(path, true)
            .is_ok_and(|resolved| lock(&self.locks).contains(&resolved))
    }

    fn append(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        self.write_file(path, contents, true)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        let resolved = self.resolve_for_write(path, false)?;
        if self.nodes().contains_key(&resolved) {
            return Err(error(io::ErrorKind::AlreadyExists, path));
        }
        let modified = self.tick();
        self.nodes().insert(
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
        let mut nodes = self.nodes();
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
        let mut nodes = self.nodes();
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
        let mut nodes = self.nodes();
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
        let nodes = self.nodes();
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
        match self.nodes().get(&resolved) {
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
        match self.nodes().get_mut(&resolved) {
            Some(Node::File { mode, .. } | Node::Directory { mode, .. }) => {
                *mode = new_mode & 0o7777;
            }
            _ => return Err(error(io::ErrorKind::NotFound, path)),
        }
        self.record(FileOperation::SetMode(resolved, new_mode & 0o7777));
        Ok(())
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.check_failure(path)?;
        let resolved = self.resolve(path, true)?;
        if self.nodes().contains_key(&resolved) {
            Ok(resolved)
        } else {
            Err(error(io::ErrorKind::NotFound, path))
        }
    }
}
