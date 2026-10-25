//! The sources of the checkout that a test image is built from, as one
//! fingerprint among the image's inputs: the build compiles `zbm` and
//! installs the runtime from the checkout, so an image built from other
//! sources is not reused.

use std::fs;
use std::hash::{DefaultHasher, Hasher};
use std::path::{Path, PathBuf};

/// What goes into an image, relative to the checkout: the build reads the
/// cargo configuration, links `config/zfsbootmenu/dracut.conf.d` into the
/// image, and compiles the vendored crates (see [`vendored`]).
const SOURCES: &[&str] = &[
    ".cargo/config.toml",
    "Cargo.toml",
    "Cargo.lock",
    "config",
    "crates",
    "legacy",
    "packaging",
    "tests/luks/vm/hooks",
];

/// Directories below `crates` that no image contains: the VM harness,
/// build output, and tests.
const NOT_IN_IMAGES: &[&str] = &["zbm-vmtest", "target", "tests", "snapshots", "benches"];

/// Whether `relative`, a path below the checkout, is left out: test code
/// of the crates, and the harness itself.
fn is_left_out(relative: &Path) -> bool {
    let mut components = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy());
    if components.next().as_deref() != Some("crates") {
        return false;
    }
    let names: Vec<_> = components.collect();
    let name = names.last().map_or("", |name| name.as_ref());
    names
        .iter()
        .any(|component| NOT_IN_IMAGES.contains(&component.as_ref()))
        || name == "tests.rs"
        || name.ends_with("_tests.rs")
        || name.ends_with(".snap")
}

/// The files below `path` (or `path` itself), relative to `root`.
fn files(root: &Path, path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(relative) = path.strip_prefix(root) else {
        return;
    };
    if is_left_out(relative) {
        return;
    }
    if path.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            files(root, &entry.path(), out);
        }
    } else if path.is_file() {
        out.push(relative.to_path_buf());
    }
}

/// What stands for `vendor/`: each vendored crate's `.cargo-checksum.json`,
/// which holds the checksum of every file of the crate (cargo checks them
/// when it builds offline), instead of hundreds of megabytes of sources.
fn vendored(repo: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(repo.join("vendor")) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path().join(".cargo-checksum.json");
        if let Ok(relative) = path.strip_prefix(repo)
            && path.is_file()
        {
            out.push(relative.to_path_buf());
        }
    }
}

/// The fingerprint of the sources in `repo` that images are built from:
/// their paths and contents.
pub fn fingerprint(repo: &Path) -> String {
    let mut paths = Vec::new();
    for source in SOURCES {
        files(repo, &repo.join(source), &mut paths);
    }
    vendored(repo, &mut paths);
    hash(repo, paths)
}

/// The fingerprint of the files below `dir`: their paths and contents.
pub fn fingerprint_of(dir: &Path) -> String {
    let mut paths = Vec::new();
    files(dir, dir, &mut paths);
    hash(dir, paths)
}

/// The hash of the files `paths` below `root`, sorted: their paths and
/// contents, and how many there are.
fn hash(root: &Path, mut paths: Vec<PathBuf>) -> String {
    paths.sort();
    let mut hasher = DefaultHasher::new();
    for path in &paths {
        hasher.write(path.to_string_lossy().as_bytes());
        hasher.write_u8(0);
        hasher.write(&fs::read(root.join(path)).unwrap_or_default());
        hasher.write_u8(0);
    }
    format!("{:016x} {} files", hasher.finish(), paths.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_follows_the_sources_but_not_the_tests() {
        let root = std::env::temp_dir().join(format!("zbm-vmtest-source-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        for (path, text) in [
            ("Cargo.lock", "lock"),
            ("crates/zbm/src/lib.rs", "one"),
            ("crates/zbm/src/lib/tests.rs", "test"),
            ("crates/zbm-vmtest/src/lib.rs", "harness"),
            ("legacy/zfsbootmenu/bin/zfsbootmenu", "menu"),
            ("docs/index.md", "docs"),
            ("config/zfsbootmenu/dracut.conf.d/omit.conf", "omit"),
            ("vendor/regex/.cargo-checksum.json", "{}"),
            ("vendor/regex/src/lib.rs", "regex"),
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let first = fingerprint(&root);
        assert!(first.ends_with(" 5 files"), "{first}");

        fs::write(root.join("crates/zbm/src/lib/tests.rs"), "other test").unwrap();
        fs::write(root.join("crates/zbm-vmtest/src/lib.rs"), "other harness").unwrap();
        fs::write(root.join("docs/index.md"), "other docs").unwrap();
        assert_eq!(fingerprint(&root), first);

        fs::write(root.join("crates/zbm/src/lib.rs"), "two").unwrap();
        assert_ne!(fingerprint(&root), first);
        fs::write(root.join("crates/zbm/src/lib.rs"), "one").unwrap();
        fs::write(root.join("legacy/zfsbootmenu/bin/zfsbootmenu"), "new menu").unwrap();
        let second = fingerprint(&root);
        assert_ne!(second, first);
        // A vendored crate counts through its checksums.
        fs::write(root.join("vendor/regex/src/lib.rs"), "other regex").unwrap();
        assert_eq!(fingerprint(&root), second);
        fs::write(root.join("vendor/regex/.cargo-checksum.json"), "{\"a\":1}").unwrap();
        assert_ne!(fingerprint(&root), second);
        fs::remove_dir_all(&root).ok();
    }
}
