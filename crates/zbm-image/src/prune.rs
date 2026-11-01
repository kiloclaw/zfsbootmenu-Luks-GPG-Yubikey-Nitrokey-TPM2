//! The graphics firmware an image keeps (`zbm image prune-firmware`): the
//! builder installs the graphics drivers with every firmware file they
//! list, then this removes, in their firmware directories, the files the
//! plan does not name, keeping what the named links point to.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use zbm_platform::{FileKind, Files};

use super::Fatal;

/// What pruning did.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Pruned {
    /// The files kept.
    pub kept: usize,
    /// The files removed.
    pub removed: usize,
    /// The files named that the image does not have, in order.
    pub missing: Vec<String>,
}

/// The files and links below `dir`, depth first and in byte order.
fn files_below(files: &dyn Files, dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut names: Vec<_> = files.read_dir(dir).unwrap_or_default();
    names.sort();
    for name in names {
        let path = dir.join(name);
        if files.read_link(&path).is_ok() {
            found.push(path);
        } else if files.is_dir(&path) {
            found.extend(files_below(files, &path));
        } else if files
            .metadata(&path)
            .is_ok_and(|metadata| metadata.kind == FileKind::File)
        {
            found.push(path);
        }
    }
    found
}

/// The firmware trees of the image at `root`: `usr/lib/firmware` and
/// `lib/firmware`, once when one is a link to the other.
fn trees(files: &dyn Files, root: &Path) -> Vec<PathBuf> {
    let mut trees: Vec<PathBuf> = Vec::new();
    for tree in ["usr/lib/firmware", "lib/firmware"].map(|tree| root.join(tree)) {
        if !files.is_dir(&tree) {
            continue;
        }
        let same = trees
            .first()
            .is_some_and(|first| files.canonicalize(first).ok() == files.canonicalize(&tree).ok());
        if !same {
            trees.push(tree);
        }
    }
    trees
}

/// The firmware's name of the file `relative`: without `.zst` or `.xz`.
fn firmware_name(relative: &Path) -> String {
    let name = relative.to_string_lossy();
    let name = name.strip_suffix(".zst").unwrap_or(&name);
    name.strip_suffix(".xz").unwrap_or(name).to_owned()
}

/// What the link `link` of the image at `root` points to, in the image.
fn link_target(files: &dyn Files, root: &Path, link: &Path) -> Option<PathBuf> {
    let target = files.read_link(link).ok()?;
    let target = if target.is_absolute() {
        root.join(target.strip_prefix("/").unwrap_or(&target))
    } else {
        link.parent()?.join(target)
    };
    let target = super::host::normalized(&target);
    let target = files.canonicalize(&target).unwrap_or(target);
    Some(if target.starts_with(root) {
        target
    } else {
        root.join(target.strip_prefix("/").unwrap_or(&target))
    })
}

/// Keeps, in the directories `dirs` of the firmware trees of the image at
/// `root`, only the files `names` name, compressed or not, and what those
/// that are links point to.
///
/// # Errors
///
/// When the image has none of `names`: linux-firmware is missing.
pub fn prune_firmware(
    files: &dyn Files,
    root: &Path,
    dirs: &[String],
    names: &[String],
) -> Result<Pruned, Fatal> {
    let mut listed: BTreeMap<&str, bool> =
        names.iter().map(|name| (name.as_str(), false)).collect();
    let mut keep: BTreeSet<PathBuf> = BTreeSet::new();
    let mut found = Vec::new();
    for tree in trees(files, root) {
        for dir in dirs {
            for file in files_below(files, &tree.join(dir)) {
                let name = firmware_name(file.strip_prefix(&tree).unwrap_or(&file));
                if let Some(seen) = listed.get_mut(name.as_str()) {
                    *seen = true;
                    keep.insert(file.clone());
                    keep.extend(link_target(files, root, &file));
                }
                found.push(file);
            }
        }
    }
    let mut pruned = Pruned::default();
    for file in found {
        if keep.contains(&file) {
            pruned.kept += 1;
        } else if files.remove_file(&file).is_ok() {
            pruned.removed += 1;
        }
    }
    pruned.missing = listed
        .into_iter()
        .filter(|(_, seen)| !seen)
        .map(|(name, _)| name.to_owned())
        .collect();
    if pruned.missing.len() == names.len() {
        return Err(
            "none of the graphics firmware is in the image; is linux-firmware installed?".into(),
        );
    }
    Ok(pruned)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeFiles;

    use super::*;

    fn strings(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn only_the_named_firmware_and_what_its_links_point_to_stays() {
        let files = FakeFiles::new();
        files
            .file("/img/usr/lib/firmware/i915/adlp_dmc.bin.zst", "a")
            .file("/img/usr/lib/firmware/i915/tgl_dmc.bin.xz", "b")
            .file("/img/usr/lib/firmware/i915/real_guc.bin", "c")
            .symlink("/img/usr/lib/firmware/i915/adlp_guc.bin", "real_guc.bin")
            .symlink(
                "/img/usr/lib/firmware/i915/abs.bin",
                "/usr/lib/firmware/i915/real_guc.bin",
            )
            .file("/img/usr/lib/firmware/amdgpu/x.bin", "d")
            .symlink("/img/lib", "usr/lib");

        let pruned = prune_firmware(
            &files,
            Path::new("/img"),
            &strings(&["i915", "xe"]),
            &strings(&[
                "i915/adlp_dmc.bin",
                "i915/adlp_guc.bin",
                "i915/abs.bin",
                "i915/none.bin",
            ]),
        )
        .unwrap();

        assert_eq!(pruned.kept, 4);
        assert_eq!(pruned.removed, 1);
        assert_eq!(pruned.missing, ["i915/none.bin"]);
        assert!(!files.exists(Path::new("/img/usr/lib/firmware/i915/tgl_dmc.bin.xz")));
        assert!(files.exists(Path::new("/img/usr/lib/firmware/i915/real_guc.bin")));
        assert!(files.exists(Path::new("/img/usr/lib/firmware/amdgpu/x.bin")));

        let none = prune_firmware(
            &files,
            Path::new("/img"),
            &strings(&["i915"]),
            &strings(&["x"]),
        );
        assert!(none.unwrap_err().contains("linux-firmware"));
    }
}
