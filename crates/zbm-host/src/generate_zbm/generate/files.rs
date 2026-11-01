//! The file operations the installation makes: directories made with their
//! parents, and copies that are whole or none.

use std::path::{Path, PathBuf};

use zbm_platform::filetest::{is_dir, strerror};

use super::{Step, Stop};
use crate::generate_zbm::perl;

/// `File::Path`'s `make_path`: creates a directory and its missing parents;
/// failing to create one is fatal.
pub(super) fn make_path(path: &str) -> Step<()> {
    if path.is_empty() || is_dir(path) {
        return Ok(());
    }
    let parent = perl::dirname(path);
    if !(is_dir(&parent) || parent == path) {
        make_path(&parent)?;
    }
    match std::fs::create_dir(path) {
        Err(error) if !is_dir(path) => Err(Stop::fatal(
            format!("mkdir {path}: {}", strerror(&error)),
            &error,
        )),
        _ => Ok(()),
    }
}

/// `File::Copy`'s `copy`: copies the contents into `dest` (into a file of the
/// same name when `dest` is a directory), with the default permissions.
/// The copy replaces `dest` whole or not at all
/// ([`crate::files::copy_over`]), keeping its mode.
pub(super) fn file_copy(source: &str, dest: &str) -> std::io::Result<()> {
    let mut dest = PathBuf::from(dest);
    if dest.is_dir() && !Path::new(source).is_dir() {
        dest = dest.join(perl::basename(source));
    }
    crate::files::copy_over(Path::new(source), &dest, crate::files::NewMode::Default)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use zbm_testkit::TempDir;

    use super::*;

    #[test]
    fn a_copy_is_whole_or_leaves_the_target_as_it_was() {
        let dir = TempDir::new("generate-zbm-copy");
        let source = dir.write("source", "new contents");
        let target = dir.write("target", "old");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let (source, target_text) = (source.display().to_string(), target.display().to_string());

        file_copy(&source, &target_text).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new contents");
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let missing = dir.join("missing").display().to_string();
        assert!(file_copy(&missing, &target_text).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new contents");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "no temporary file is left: {names:?}");
        let into = dir.join("into");
        std::fs::create_dir(&into).unwrap();
        file_copy(&source, &into.display().to_string()).unwrap();
        assert_eq!(
            std::fs::read_to_string(into.join("source")).unwrap(),
            "new contents"
        );
    }
}
