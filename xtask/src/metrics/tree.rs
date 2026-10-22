//! Finding a package's source files by following its module tree.
//!
//! The walk starts at each target's root file (`src/lib.rs`, `src/main.rs`,
//! `tests/*.rs`) and follows `mod name;` declarations the way the compiler
//! does, so that it knows which files are test code: a file is test code
//! when it is an integration test or a module declared under `#[cfg(test)]`.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::metrics::scan::{FileScan, scan_file};
use crate::workspace::{Package, TargetKind};

/// One scanned source file.
#[derive(Clone, Debug)]
pub struct ScannedFile {
    /// The file.
    pub path: PathBuf,
    /// What it holds.
    pub scan: FileScan,
}

/// A file still to scan.
struct Pending {
    /// The file.
    path: PathBuf,
    /// The directory its out-of-line child modules live in.
    children: PathBuf,
    /// Whether the whole file is test code.
    test_code: bool,
}

/// Scans every source file of `package`.
///
/// # Errors
///
/// [`Error::Io`] when a file cannot be read, [`Error::Parse`] when it is not
/// valid Rust, [`Error::Format`] when a declared module has no file.
pub fn scan_package(package: &Package, legacy_names: &HashSet<String>) -> Result<Vec<ScannedFile>> {
    let mut queue: VecDeque<Pending> = package
        .targets
        .iter()
        .filter(|target| target.kind != TargetKind::Other)
        .map(|target| Pending {
            children: target
                .src
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            path: target.src.clone(),
            test_code: target.kind == TargetKind::Test,
        })
        .collect();
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    while let Some(pending) = queue.pop_front() {
        if !seen.insert(pending.path.clone()) {
            continue;
        }
        let scan = scan_path(&pending.path, pending.test_code, legacy_names)?;
        for decl in &scan.modules {
            let mut dir = pending.children.clone();
            dir.extend(&decl.inline);
            let (path, children) = module_file(&dir, &decl.name).ok_or_else(|| {
                Error::Format(format!(
                    "{}: module {} has no file in {}",
                    pending.path.display(),
                    decl.name,
                    dir.display()
                ))
            })?;
            queue.push_back(Pending {
                path,
                children,
                test_code: decl.test_code,
            });
        }
        files.push(ScannedFile {
            path: pending.path,
            scan,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Reads, parses and scans one file.
fn scan_path(path: &Path, test_code: bool, legacy_names: &HashSet<String>) -> Result<FileScan> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Error::io(format!("reading {}", path.display()), error))?;
    let file = syn::parse_file(&text).map_err(|source| Error::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(scan_file(
        &file,
        text.lines().count(),
        test_code,
        legacy_names,
    ))
}

/// The file of module `name` declared in `dir`, and the directory of its
/// own children: `dir/name.rs` or `dir/name/mod.rs`, children in
/// `dir/name/` either way.
fn module_file(dir: &Path, name: &str) -> Option<(PathBuf, PathBuf)> {
    let children = dir.join(name);
    let flat = dir.join(format!("{name}.rs"));
    if flat.is_file() {
        return Some((flat, children));
    }
    let nested = children.join("mod.rs");
    nested.is_file().then_some((nested, children))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Target;

    /// A package with the given files, in a directory of its own below the
    /// system's temporary directory.
    fn package(name: &str, files: &[(&str, &str)]) -> Package {
        let dir = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
        for (path, text) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        Package {
            name: name.to_owned(),
            dir: dir.clone(),
            targets: vec![
                Target {
                    kind: TargetKind::Lib,
                    src: dir.join("src/lib.rs"),
                },
                Target {
                    kind: TargetKind::Test,
                    src: dir.join("tests/it.rs"),
                },
            ],
            dependencies: Vec::new(),
        }
    }

    fn relative(package: &Package, files: &[ScannedFile]) -> Vec<(String, bool)> {
        files
            .iter()
            .map(|file| {
                let path = file.path.strip_prefix(&package.dir).unwrap();
                let test = file.scan.functions.iter().all(|f| f.role.is_test_code());
                (path.display().to_string(), test)
            })
            .collect()
    }

    #[test]
    fn modules_are_followed_and_test_modules_are_test_code() {
        let package = package(
            "tree-follow",
            &[
                (
                    "src/lib.rs",
                    "mod a;\n#[cfg(test)]\nmod testing;\nfn root() {}\n",
                ),
                ("src/a.rs", "mod b;\nfn a() {}\n"),
                ("src/a/b/mod.rs", "fn b() {}\n"),
                ("src/testing.rs", "fn helper() {}\n"),
                ("tests/it.rs", "fn it() {}\n"),
            ],
        );

        let files = scan_package(&package, &HashSet::new()).unwrap();

        assert_eq!(
            relative(&package, &files),
            [
                ("src/a/b/mod.rs".to_owned(), false),
                ("src/a.rs".to_owned(), false),
                ("src/lib.rs".to_owned(), false),
                ("src/testing.rs".to_owned(), true),
                ("tests/it.rs".to_owned(), true),
            ]
        );
    }

    #[test]
    fn a_module_without_a_file_is_an_error() {
        let package = package(
            "tree-missing",
            &[("src/lib.rs", "mod gone;\n"), ("tests/it.rs", "")],
        );

        let error = scan_package(&package, &HashSet::new()).unwrap_err();

        assert!(error.to_string().contains("module gone has no file"));
    }
}
