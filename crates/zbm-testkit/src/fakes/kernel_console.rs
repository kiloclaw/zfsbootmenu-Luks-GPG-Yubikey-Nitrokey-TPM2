//! Fonts loaded on virtual terminals and kernel modules inserted, recorded
//! (ROADMAP.md CD5, CD4).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use zbm_platform::{ConsoleFonts, KernelFont, ModuleLoader};

use crate::sync::lock;

/// A [`ConsoleFonts`] that records each font loaded, and refuses fonts
/// taller than it takes.
#[derive(Debug, Default)]
pub struct FakeFonts {
    loaded: Mutex<Vec<(PathBuf, KernelFont)>>,
    tallest: Mutex<Option<u32>>,
}

impl FakeFonts {
    /// Refuses fonts taller than `height` pixels, as a kernel that takes
    /// no larger font.
    pub fn refuse_taller_than(&self, height: u32) {
        *lock(&self.tallest) = Some(height);
    }

    /// The fonts loaded: the terminal, the cell's size and the glyphs.
    pub fn loaded(&self) -> Vec<(PathBuf, (u32, u32), u32)> {
        lock(&self.loaded)
            .iter()
            .map(|(path, font)| (path.clone(), (font.width, font.height), font.count))
            .collect()
    }
}

impl ConsoleFonts for FakeFonts {
    fn load(&self, console: &Path, font: &KernelFont) -> io::Result<()> {
        if lock(&self.tallest).is_some_and(|tallest| font.height > tallest) {
            return Err(io::Error::from_raw_os_error(22));
        }
        lock(&self.loaded).push((console.to_owned(), font.clone()));
        Ok(())
    }
}

/// A [`ModuleLoader`] that records each module inserted, with its
/// parameters, and fails those it is told to; a module inserted again
/// fails with `EEXIST`, as the kernel answers.
#[derive(Debug, Default)]
pub struct FakeModules {
    inserted: Mutex<Vec<(PathBuf, String)>>,
    failing: Mutex<BTreeMap<PathBuf, i32>>,
}

impl FakeModules {
    /// Fails the insertion of `path` with the OS error `errno`: `EEXIST`
    /// (17) for a module loaded already, `EINVAL` (22) for one the kernel
    /// cannot decompress, `ENOEXEC` (8) for no module.
    pub fn fail(&self, path: impl Into<PathBuf>, errno: i32) {
        lock(&self.failing).insert(path.into(), errno);
    }

    /// The modules inserted, in order: the file and its parameters.
    pub fn inserted(&self) -> Vec<(PathBuf, String)> {
        lock(&self.inserted).clone()
    }
}

impl ModuleLoader for FakeModules {
    fn insert(&self, path: &Path, parameters: &str) -> io::Result<()> {
        if let Some(errno) = lock(&self.failing).get(path) {
            return Err(io::Error::from_raw_os_error(*errno));
        }
        let mut inserted = lock(&self.inserted);
        if inserted.iter().any(|(done, _)| done == path) {
            return Err(io::Error::from_raw_os_error(17));
        }
        inserted.push((path.to_owned(), parameters.to_owned()));
        Ok(())
    }
}
