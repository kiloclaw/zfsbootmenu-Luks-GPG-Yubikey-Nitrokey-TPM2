//! Real files with some absolute paths moved elsewhere, so that code that
//! writes `/etc/hostid` or a module parameter can run on a test machine and
//! be compared with the legacy code that writes the same files.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use zbm_platform::files::{FileLock, Files, LockWait, Metadata, RealFiles};

/// Real files, with some absolute paths moved elsewhere.
#[derive(Clone)]
pub struct Redirect {
    moves: Vec<(PathBuf, PathBuf)>,
}

impl Redirect {
    /// Real files where each `(from, to)` pair moves `from` to `to`.
    pub fn new(moves: &[(&str, &Path)]) -> Redirect {
        Redirect {
            moves: moves
                .iter()
                .map(|(from, to)| (PathBuf::from(from), to.to_path_buf()))
                .collect(),
        }
    }

    fn map<'a>(&'a self, path: &'a Path) -> &'a Path {
        self.moves
            .iter()
            .find(|(from, _)| from == path)
            .map_or(path, |(_, to)| to)
    }
}

impl Files for Redirect {
    fn shared(&self) -> std::sync::Arc<dyn Files + Send + Sync> {
        std::sync::Arc::new(self.clone())
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        RealFiles.read(self.map(path))
    }
    fn read_prefix(&self, path: &Path, len: usize) -> io::Result<Vec<u8>> {
        RealFiles.read_prefix(self.map(path), len)
    }
    fn read_available(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        RealFiles.read_available(self.map(path), limit)
    }
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        RealFiles.write(self.map(path), contents)
    }
    fn write_private(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        RealFiles.write_private(self.map(path), contents)
    }
    fn lock(&self, path: &Path, wait: LockWait) -> io::Result<Option<FileLock>> {
        RealFiles.lock(self.map(path), wait)
    }
    fn is_locked(&self, path: &Path) -> bool {
        RealFiles.is_locked(self.map(path))
    }
    fn append(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        RealFiles.append(self.map(path), contents)
    }
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        RealFiles.create_dir(self.map(path))
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        RealFiles.create_dir_all(self.map(path))
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        RealFiles.remove_file(self.map(path))
    }
    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        RealFiles.remove_dir(self.map(path))
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        RealFiles.rename(self.map(from), self.map(to))
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<OsString>> {
        RealFiles.read_dir(self.map(path))
    }
    fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        RealFiles.metadata(self.map(path))
    }
    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        RealFiles.set_mode(self.map(path), mode)
    }
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        RealFiles.canonicalize(self.map(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TempDir;

    #[test]
    fn redirect_moves_only_the_listed_paths() {
        let dir = TempDir::new("zbm-redirect");
        let moved = dir.join("moved");
        let files = Redirect::new(&[("/no/such/file", &moved)]);
        let from = Path::new("/no/such/file");
        files.write(from, b"a").unwrap();
        files.append(from, b"b").unwrap();
        assert_eq!(files.read_prefix(from, 1).unwrap(), b"a");
        assert_eq!(std::fs::read(&moved).unwrap(), b"ab");
        let renamed = dir.join("renamed");
        files.rename(from, &renamed).unwrap();
        assert_eq!(files.canonicalize(&renamed).unwrap(), renamed);
        files.set_mode(&renamed, 0o600).unwrap();
        files.remove_file(&renamed).unwrap();
        let sub = dir.join("a/b");
        files.create_dir_all(&sub).unwrap();
        files.create_dir(&sub.join("c")).unwrap();
        files.remove_dir(&sub.join("c")).unwrap();
        assert_eq!(files.read_dir(&sub).unwrap().len(), 0);
        assert!(files.shared().exists(&sub));
    }
}
