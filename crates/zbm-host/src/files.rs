//! Files replaced whole or not at all, for both host tools: the new contents
//! go to a temporary file beside the target, which is written to the disk
//! and then renamed over it, so that the target is the old file or the new
//! one, even after a failed write, a full disk or a power cut. A failure
//! leaves no temporary file.

#![allow(
    clippy::disallowed_methods,
    reason = "the one place the host tools replace files of the host"
)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

/// The mode a replaced file gets when there was none before.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NewMode {
    /// The mode a new file gets: the default less the umask.
    Default,
    /// This mode, as `cp` gives a new file its source's.
    Of(u32),
}

/// Replaces `dest` with what `fill` writes, keeping the mode of `dest` when
/// it exists, else giving it `new_mode`.
///
/// # Errors
///
/// When the temporary file cannot be made, filled, written to the disk or
/// renamed; `dest` is then as it was.
pub fn replace(
    dest: &Path,
    new_mode: NewMode,
    fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let directory = dest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (temp, mut staged) = temporary(directory, dest)?;
    let mode = match std::fs::metadata(dest) {
        Ok(existing) => Some(existing.permissions().mode()),
        Err(_) => match new_mode {
            NewMode::Default => None,
            NewMode::Of(mode) => Some(mode),
        },
    };
    let written = mode
        .map_or(Ok(()), |mode| {
            staged.set_permissions(std::fs::Permissions::from_mode(mode & 0o7777))
        })
        .and_then(|()| fill(&mut staged))
        .and_then(|()| staged.sync_all())
        .and_then(|()| std::fs::rename(&temp, dest));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
        return written;
    }
    // The rename itself reaches the disk with the directory.
    if let Ok(directory) = std::fs::File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}

/// Replaces `dest` with a copy of `source` ([`replace`]).
///
/// # Errors
///
/// When `source` cannot be read, or [`replace`] fails.
pub fn copy_over(source: &Path, dest: &Path, new_mode: NewMode) -> std::io::Result<()> {
    let mut from = std::fs::File::open(source)?;
    replace(dest, new_mode, |to| {
        std::io::copy(&mut from, to).map(|_| ())
    })
}

/// A new file in `directory`, named after `dest`, to fill.
fn temporary(directory: &Path, dest: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
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
            Ok(file) => return Ok((temp, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use zbm_testkit::TempDir;

    use super::*;

    fn names(dir: &TempDir) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_copy_is_whole_or_leaves_the_target_as_it_was() {
        let dir = TempDir::new("zbm-host-files");
        let source = dir.write("source", "new contents");
        let target = dir.write("target", "old");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();

        copy_over(&source, &target, NewMode::Of(0o644)).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new contents");
        assert_eq!(mode(&target), 0o600, "the target keeps its mode");

        assert!(copy_over(&dir.join("missing"), &target, NewMode::Default).is_err());
        let failed = replace(&target, NewMode::Default, |file| {
            file.write_all(b"partial")?;
            Err(std::io::Error::other("full disk"))
        });
        assert!(failed.is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new contents");
        assert_eq!(
            names(&dir),
            ["source", "target"],
            "no temporary file is left"
        );
    }

    #[test]
    fn a_new_file_gets_the_mode_asked() {
        let dir = TempDir::new("zbm-host-files");
        let source = dir.write("source", "image");
        let new = dir.join("new");
        copy_over(&source, &new, NewMode::Of(0o640)).unwrap();
        assert_eq!(
            (std::fs::read_to_string(&new).unwrap(), mode(&new)),
            ("image".to_owned(), 0o640)
        );
        assert!(copy_over(&source, &dir.join("none/new"), NewMode::Default).is_err());
    }
}
