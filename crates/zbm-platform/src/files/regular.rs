//! Reading a regular file of bounded size ([`Files::read_regular`]): never
//! through a symbolic link at its last name, never from a pipe, device or
//! socket, and never more than the limit and one byte.

use std::io::{self, Read};
use std::path::Path;

use rustix::fs::{FileType, Mode, OFlags};

use super::{FileKind, Files};

/// The error for a file that is no regular file.
fn not_regular() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "not a regular file")
}

/// The error for a file of `len` bytes, over `limit`.
fn too_large(len: u64, limit: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::FileTooLarge,
        format!("{len} bytes, more than {limit}"),
    )
}

/// `len` as a buffer's length, the largest one where it does not fit.
fn length(len: u64) -> usize {
    usize::try_from(len).unwrap_or(usize::MAX)
}

/// `data` when it holds at most `limit` bytes.
fn bounded(data: Vec<u8>, limit: u64) -> io::Result<Vec<u8>> {
    let len = u64::try_from(data.len()).unwrap_or(u64::MAX);
    if len > limit {
        return Err(too_large(len, limit));
    }
    Ok(data)
}

/// [`Files::read_regular`] through the other methods of `files`: the last
/// name must be no link, then a regular file of at most `limit` bytes, of
/// which at most `limit + 1` are read.
pub(super) fn through<F: Files + ?Sized>(
    files: &F,
    path: &Path,
    limit: u64,
) -> io::Result<Vec<u8>> {
    match files.read_link(path) {
        Ok(_) => return Err(rustix::io::Errno::LOOP.into()),
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
        Err(error) => return Err(error),
    }
    let metadata = files.metadata(path)?;
    if metadata.kind != FileKind::File {
        return Err(not_regular());
    }
    if metadata.len > limit {
        return Err(too_large(metadata.len, limit));
    }
    bounded(
        files.read_prefix(path, length(limit.saturating_add(1)))?,
        limit,
    )
}

/// [`Files::read_regular`] of the real files: the last name opened with
/// `O_NOFOLLOW | O_NONBLOCK`, so that a link there fails with `ELOOP` and a
/// pipe opens without a writer, then the opened file checked with `fstat`,
/// and a buffer sized from its length.
pub(super) fn opened(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let descriptor = rustix::fs::open(path, flags, Mode::empty())?;
    let stat = rustix::fs::fstat(&descriptor)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(not_regular());
    }
    let len = u64::try_from(stat.st_size).unwrap_or(0);
    if len > limit {
        return Err(too_large(len, limit));
    }
    let mut data = Vec::with_capacity(length(len));
    std::fs::File::from(descriptor)
        .take(limit.saturating_add(1))
        .read_to_end(&mut data)?;
    bounded(data, limit)
}
