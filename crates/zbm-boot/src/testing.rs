//! Helpers for the tests of this crate.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use zbm_system::files::{Files, Metadata, RealFiles};

pub use zbm_testkit::legacy::{LINE_LOG_STUBS, stub_lines};

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
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        RealFiles.write(self.map(path), contents)
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
