//! The file operations the installation makes: directories made with their
//! parents, and copies that are whole or none.

use std::io::Write;
use std::path::{Path, PathBuf};

use zbm_platform::filetest::{is_dir, strerror};

use super::{Step, Stop};
use crate::generate_zbm::perl;

/// File::Path's `make_path`: creates a directory and its missing parents;
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

/// File::Copy's `copy`: copies the contents into `dest` (into a file of the
/// same name when `dest` is a directory), with the default permissions.
/// The contents go to a temporary file beside `dest`, which is written to
/// the disk and then renamed: `dest` is either as it was or whole, even
/// after a failed write, a full disk or a power cut. A failure leaves no
/// temporary file.
pub(super) fn file_copy(source: &str, dest: &str) -> std::io::Result<()> {
    let mut dest = PathBuf::from(dest);
    if dest.is_dir() && !Path::new(source).is_dir() {
        dest = dest.join(perl::basename(source));
    }
    let mut from = std::fs::File::open(source)?;
    let directory = dest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (temp, to) = copy_target(directory, &dest)?;
    let copied = copy_into(&mut from, to).and_then(|()| std::fs::rename(&temp, &dest));
    if copied.is_err() {
        let _ = std::fs::remove_file(&temp);
        return copied;
    }
    // The rename itself reaches the disk with the directory.
    if let Ok(directory) = std::fs::File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}

/// A new file in `directory` to copy `dest` into, with the mode of `dest`
/// when it exists, else the mode a new file gets (the default less the
/// umask), as copying into `dest` would give it.
fn copy_target(directory: &Path, dest: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    let name = dest
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let mut attempt = 0_u32;
    loop {
        let temp = directory.join(format!(".{name}.{}.{attempt}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => {
                // A file copied over keeps its mode, as `copy` truncating
                // it would.
                if let Ok(existing) = std::fs::metadata(dest) {
                    let _ = file.set_permissions(existing.permissions());
                }
                return Ok((temp, file));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Copies `from` into the file `to`, and writes it to the disk; a failed
/// write, or one the disk reports late, is an error.
fn copy_into(from: &mut std::fs::File, mut to: std::fs::File) -> std::io::Result<()> {
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let count = std::io::Read::read(from, &mut buffer)?;
        if count == 0 {
            break;
        }
        to.write_all(&buffer[..count])?;
    }
    to.sync_all()
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
