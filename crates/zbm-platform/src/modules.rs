//! Kernel modules (ROADMAP.md CD4): a module's file handed to the kernel,
//! as `insmod` hands it, and the [`index`] of the kernel's modules, which
//! says which files, in which order, with which parameters.

use std::fs::File;
use std::io;
use std::path::Path;

pub mod index;

/// Inserting modules into the kernel.
pub trait ModuleLoader: Send + Sync {
    /// Inserts the module in the file `path` with `parameters`; a file
    /// ending in `.gz`, `.xz` or `.zst` the kernel decompresses.
    ///
    /// # Errors
    ///
    /// The kernel's: `AlreadyExists` for a module loaded already; others
    /// for a file it cannot load, or cannot decompress.
    fn insert(&self, path: &Path, parameters: &str) -> io::Result<()>;
}

/// Whether the module file `path` is compressed, by its name.
pub fn is_compressed(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "gz" | "xz" | "zst"))
}

/// The kernel's `finit_module`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemModules;

impl ModuleLoader for SystemModules {
    #[allow(
        clippy::disallowed_methods,
        reason = "the module's file is handed to the kernel open, which Files cannot"
    )]
    fn insert(&self, path: &Path, parameters: &str) -> io::Result<()> {
        let file = File::open(path)?;
        zbm_sys::module::insert(&file, parameters, is_compressed(path))
    }
}

/// No module loading, for programs that load none.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoModules;

impl ModuleLoader for NoModules {
    fn insert(&self, _path: &Path, _parameters: &str) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_modules_are_told_by_their_names() {
        assert!(is_compressed(Path::new(
            "/lib/modules/6.12/kernel/zfs.ko.zst"
        )));
        assert!(is_compressed(Path::new("spl.ko.xz")));
        assert!(is_compressed(Path::new("spl.ko.gz")));
        assert!(!is_compressed(Path::new("spl.ko")));
        assert!(SystemModules.insert(Path::new("/dev/null"), "").is_err());
        assert!(
            SystemModules
                .insert(Path::new("/nonexistent/spl.ko"), "")
                .is_err()
        );
        let error = NoModules.insert(Path::new("spl.ko"), "").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }
}
