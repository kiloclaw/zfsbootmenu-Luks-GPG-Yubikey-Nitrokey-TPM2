//! File trees: copying, restricting and removing them, in process instead
//! of by running `cp -a SOURCE/. TARGET/`, `chmod -R go-rwx PATH` and
//! `rm -rf PATH`, through the [`Files`] port, so that tests run them on a
//! fake file system.
//!
//! The operations follow symbolic links where `cp -a` and `chmod -R` would
//! keep or skip them: [`Files`] reports what a path names after following
//! links. ZFSBootMenu uses them on directories it builds itself, such as a
//! GnuPG home copied from the image, which hold directories and regular
//! files only; a link loop ends with an error at [`MAX_DEPTH`] levels.
//! Sockets, devices and pipes are not copied.

use std::io;
use std::path::Path;

use crate::files::{FileKind, Files};

/// How deep [`copy_into`] and [`clear_mode_bits`] go below the path they are
/// given.
pub const MAX_DEPTH: usize = 64;

/// The error for a tree deeper than [`MAX_DEPTH`].
fn too_deep(path: &Path) -> io::Error {
    io::Error::other(format!(
        "{} is more than {MAX_DEPTH} directories deep",
        path.display()
    ))
}

/// Copies what the directory `source` holds into the directory `target`, as
/// `cp -a SOURCE/. TARGET/` does: directories and regular files with their
/// permission bits, replacing files of the same name.
///
/// # Errors
///
/// The first error of the file system: `source` or `target` cannot be read
/// or written, or the tree is deeper than [`MAX_DEPTH`].
pub fn copy_into(files: &dyn Files, source: &Path, target: &Path) -> io::Result<()> {
    copy_entries(files, source, target, 0)
}

/// Copies the entries of `source` into `target`, `depth` levels down.
fn copy_entries(files: &dyn Files, source: &Path, target: &Path, depth: usize) -> io::Result<()> {
    if depth > MAX_DEPTH {
        return Err(too_deep(source));
    }
    for name in files.read_dir(source)? {
        let from = source.join(&name);
        let to = target.join(&name);
        let metadata = files.metadata(&from)?;
        match metadata.kind {
            FileKind::Directory => {
                if !files.is_dir(&to) {
                    files.create_dir(&to)?;
                }
                copy_entries(files, &from, &to, depth + 1)?;
                files.set_mode(&to, metadata.mode)?;
            }
            FileKind::File => {
                files.copy(&from, &to)?;
                files.set_mode(&to, metadata.mode)?;
            }
            FileKind::BlockDevice | FileKind::CharDevice | FileKind::Socket | FileKind::Other => {}
        }
    }
    Ok(())
}

/// Clears the permission bits `bits` of `path` and of everything below it, as
/// `chmod -R` does with a mode such as `go-rwx` (`bits` `0o077`).
///
/// # Errors
///
/// The first error of the file system, or a tree deeper than [`MAX_DEPTH`].
pub fn clear_mode_bits(files: &dyn Files, path: &Path, bits: u32) -> io::Result<()> {
    clear_below(files, path, bits, 0)
}

/// Clears `bits` on `path` and below, `depth` levels down.
fn clear_below(files: &dyn Files, path: &Path, bits: u32, depth: usize) -> io::Result<()> {
    if depth > MAX_DEPTH {
        return Err(too_deep(path));
    }
    let metadata = files.metadata(path)?;
    files.set_mode(path, metadata.mode & !bits)?;
    if metadata.kind == FileKind::Directory {
        for name in files.read_dir(path)? {
            clear_below(files, &path.join(name), bits, depth + 1)?;
        }
    }
    Ok(())
}

/// Removes `path` and, if it is a directory, everything in it, as `rm -rf`
/// does: a missing path is not an error, and a symbolic link is removed, not
/// followed. Like `rm -rf`, it goes on after a failure and reports the first
/// one.
///
/// # Errors
///
/// The first error of the file system, such as a file that cannot be removed.
pub fn remove_tree(files: &dyn Files, path: &Path) -> io::Result<()> {
    let refused = match files.remove_file(path) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => error,
    };
    if !files.is_dir(path) {
        return Err(refused);
    }
    let mut first = None;
    for name in files.read_dir(path)? {
        if let Err(error) = remove_tree(files, &path.join(name)) {
            first.get_or_insert(error);
        }
    }
    let removed = files.remove_dir(path);
    match first {
        Some(error) => Err(error),
        None => removed,
    }
}
