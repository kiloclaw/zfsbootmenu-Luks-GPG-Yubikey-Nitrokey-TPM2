//! Files and directories.
//!
//! The runtime reaches the state directory, `/proc`, `/sys`, `/dev/kmsg`,
//! device nodes, mount points and hook directories through [`Files`]. The real
//! implementation is [`RealFiles`]; tests use the in-memory fake of
//! `zbm-testkit`, which records every write.

#![allow(
    clippy::disallowed_methods,
    reason = "RealFiles is where files are read and written"
)]

use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use rustix::fs::FlockOperation;

/// `flock` of `file`, again when a signal interrupted it; whether the lock
/// was taken, `false` when a non-blocking `operation` found it held.
fn flock_retrying(file: &std::fs::File, operation: FlockOperation) -> io::Result<bool> {
    loop {
        match rustix::fs::flock(file, operation) {
            Ok(()) => return Ok(true),
            Err(rustix::io::Errno::INTR) => {}
            Err(rustix::io::Errno::WOULDBLOCK) => return Ok(false),
            Err(error) => return Err(error.into()),
        }
    }
}

/// What kind of file a path names, after following symbolic links.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A block device, which the shell's `-b` test finds.
    BlockDevice,
    /// A character device, which the shell's `-c` test finds.
    CharDevice,
    /// A socket, which the shell's `-S` test finds.
    Socket,
    /// Anything else, such as a pipe.
    Other,
}

/// What [`Files::metadata`] reports about a file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metadata {
    /// The kind of file.
    pub kind: FileKind,
    /// The permission bits, `0o7777` at most.
    pub mode: u32,
    /// The size in bytes.
    pub len: u64,
    /// The time of the last modification.
    pub modified: SystemTime,
}

impl Metadata {
    /// Whether any execute bit is set, which is what the legacy code's `-x`
    /// test means for the root user that runs it.
    pub fn is_executable(&self) -> bool {
        self.mode & 0o111 != 0
    }
}

/// How [`Files::lock`] waits for a lock another holder has.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockWait {
    /// Until the other holder lets go.
    Block,
    /// Not at all: the lock is not taken.
    Try,
}

/// An advisory lock on a file (`flock`), held until it is dropped. The
/// kernel releases it when its process ends, killed or not, so a lock is
/// never left behind by a process that is gone.
pub struct FileLock {
    _held: Box<dyn std::any::Any + Send>,
}

impl FileLock {
    /// A lock that `held` keeps until it is dropped: the open file of a real
    /// lock, or the entry of a fake one.
    pub fn new(held: impl std::any::Any + Send) -> FileLock {
        FileLock {
            _held: Box::new(held),
        }
    }
}

impl std::fmt::Debug for FileLock {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FileLock")
    }
}

/// File system operations, with the semantics of the shell commands the legacy
/// code uses for them.
pub trait Files {
    /// Reads a whole file (`cat`).
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Reads at most `len` bytes from the start of a file, as `head -c <len>`
    /// does; for devices such as `/dev/urandom`, which never end.
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn read_prefix(&self, path: &Path, len: usize) -> io::Result<Vec<u8>>;

    /// Reads what a device such as `/dev/kmsg` holds now, at most `limit`
    /// bytes: a read of it waits once it reached the end, which this does
    /// not. A plain file is read whole, as [`Files::read`] does.
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn read_available(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        let mut data = self.read(path)?;
        data.truncate(limit);
        Ok(data)
    }

    /// Creates or truncates a file and writes `contents` (`> file`).
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()>;

    /// Creates a new file for a secret, such as a cached key, and writes
    /// `contents`: mode 0600 whatever the umask, and never over a file that
    /// exists or through a symbolic link (`O_CREAT | O_EXCL`, which does not
    /// follow a link at the last component), so that another process cannot
    /// make the write land elsewhere.
    ///
    /// # Errors
    ///
    /// `AlreadyExists` when the path exists, even as a link, and the error of
    /// the operation otherwise.
    fn write_private(&self, path: &Path, contents: &[u8]) -> io::Result<()>;

    /// Takes the exclusive lock of the file `path`, created with mode 0600
    /// when it is missing; waits for another holder as `wait` says.
    ///
    /// # Errors
    ///
    /// The error of opening or locking the file. With [`LockWait::Try`], a
    /// lock that another holder has is `Ok(None)`, not an error.
    fn lock(&self, path: &Path, wait: LockWait) -> io::Result<Option<FileLock>>;

    /// Whether another holder has the lock of the file `path`; a file that
    /// does not exist is not locked.
    fn is_locked(&self, path: &Path) -> bool;

    /// Appends `contents` to a file, creating it if needed, with one write
    /// (`>> file`). Each write to `/dev/kmsg` becomes one log record.
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn append(&self, path: &Path, contents: &[u8]) -> io::Result<()>;

    /// Creates one directory, failing if it exists (`mkdir`), which makes it
    /// usable as a lock.
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn create_dir(&self, path: &Path) -> io::Result<()>;

    /// Creates a directory and its parents (`mkdir -p`).
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Removes a file (`rm`).
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    /// Removes an empty directory (`rmdir`).
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn remove_dir(&self, path: &Path) -> io::Result<()>;

    /// Renames a file, replacing the target (`mv`).
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// The names in a directory, sorted by their bytes, as a glob sorts them in
    /// the C locale.
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn read_dir(&self, path: &Path) -> io::Result<Vec<OsString>>;

    /// What a path names, following symbolic links.
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn metadata(&self, path: &Path) -> io::Result<Metadata>;

    /// Sets the permission bits (`chmod`).
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()>;

    /// The absolute path without symbolic links, `.` or `..` (`readlink -f` of
    /// an existing path).
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;

    /// Reads a whole file as text, replacing invalid UTF-8.
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.read(path)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Whether the path exists (`[ -e ]`).
    fn exists(&self, path: &Path) -> bool {
        self.metadata(path).is_ok()
    }

    /// Whether the path is a regular file (`[ -f ]`).
    fn is_file(&self, path: &Path) -> bool {
        self.metadata(path)
            .is_ok_and(|metadata| metadata.kind == FileKind::File)
    }

    /// Whether the path is a directory (`[ -d ]`).
    fn is_dir(&self, path: &Path) -> bool {
        self.metadata(path)
            .is_ok_and(|metadata| metadata.kind == FileKind::Directory)
    }

    /// Replaces a file's contents so that a reader in another process sees the
    /// old or the new contents, never a partial file: the contents go to a
    /// temporary file in the same directory, which is renamed over the target.
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn replace(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let mut temporary = OsString::from(".");
        temporary.push(name);
        temporary.push(format!(".{}.tmp", std::process::id()));
        let temporary = path.with_file_name(temporary);
        self.write(&temporary, contents)?;
        self.rename(&temporary, path).inspect_err(|_| {
            let _ = self.remove_file(&temporary);
        })
    }

    /// An owned handle to the same files that another thread may use: the
    /// kernel log's `tracing` layer writes through it.
    fn shared(&self) -> Arc<dyn Files + Send + Sync>;

    /// Mounts the ZFS filesystem or snapshot `source` on `target` with the
    /// `mount(2)` system call, read-only when `read_only`, as `mount.zfs`
    /// mounts it. Only the real files do; the others refuse, and the caller
    /// runs `mount` instead.
    ///
    /// # Errors
    ///
    /// The error of the system call; `Unsupported` where there is none.
    fn mount_zfs(&self, source: &str, target: &Path, read_only: bool) -> io::Result<()> {
        let _ = (source, target, read_only);
        Err(io::ErrorKind::Unsupported.into())
    }

    /// Unmounts `target` with the `umount2(2)` system call. Only the real
    /// files do; the others refuse, and the caller runs `umount` instead.
    ///
    /// # Errors
    ///
    /// The error of the system call; `Unsupported` where there is none.
    fn unmount(&self, target: &Path) -> io::Result<()> {
        let _ = target;
        Err(io::ErrorKind::Unsupported.into())
    }

    /// Copies a file's contents (`cp` without options).
    ///
    /// # Errors
    ///
    /// The error of the operation, such as `NotFound` for a missing path.
    fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
        let contents = self.read(from)?;
        self.write(to, &contents)
    }
}

/// The real file system.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealFiles;

impl Files for RealFiles {
    fn shared(&self) -> Arc<dyn Files + Send + Sync> {
        Arc::new(RealFiles)
    }

    fn mount_zfs(&self, source: &str, target: &Path, read_only: bool) -> io::Result<()> {
        // `mount.zfs` tells the module the mount point, which the
        // automounted snapshots of `.zfs` need.
        let mut data = b"mntpoint=".to_vec();
        data.extend_from_slice(target.as_os_str().as_encoded_bytes());
        let data = std::ffi::CString::new(data)?;
        let flags = if read_only {
            rustix::mount::MountFlags::RDONLY
        } else {
            rustix::mount::MountFlags::empty()
        };
        rustix::mount::mount(source, target, "zfs", flags, data.as_c_str())?;
        Ok(())
    }

    fn unmount(&self, target: &Path) -> io::Result<()> {
        rustix::mount::unmount(target, rustix::mount::UnmountFlags::empty())?;
        Ok(())
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn read_prefix(&self, path: &Path, len: usize) -> io::Result<Vec<u8>> {
        use std::io::Read;
        let mut data = Vec::with_capacity(len);
        std::fs::File::open(path)?
            .take(len as u64)
            .read_to_end(&mut data)?;
        Ok(data)
    }

    fn read_available(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt as _;
        let nonblocking = rustix::fs::OFlags::NONBLOCK.bits();
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(i32::try_from(nonblocking).unwrap_or(0))
            .open(path)?;
        // `/dev/kmsg` gives one record a read, and refuses a buffer too
        // small for it; a record has at most about 1 KiB of text and 8 KiB
        // with its dictionary.
        let mut buffer = vec![0_u8; 16 << 10];
        let mut data = Vec::new();
        while data.len() < limit {
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => data.extend_from_slice(buffer.get(..read).unwrap_or_default()),
                Err(error) => match error.kind() {
                    io::ErrorKind::WouldBlock => break,
                    // A record overwritten while it was read is gone: the
                    // next read gives the oldest record left.
                    io::ErrorKind::BrokenPipe | io::ErrorKind::Interrupted => {}
                    _ => return Err(error),
                },
            }
        }
        data.truncate(limit);
        Ok(data)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    fn write_private(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents)
    }

    fn lock(&self, path: &Path, wait: LockWait) -> io::Result<Option<FileLock>> {
        use std::os::unix::fs::OpenOptionsExt as _;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)?;
        let operation = match wait {
            LockWait::Block => FlockOperation::LockExclusive,
            LockWait::Try => FlockOperation::NonBlockingLockExclusive,
        };
        Ok(flock_retrying(&file, operation)?.then(|| FileLock::new(file)))
    }

    fn is_locked(&self, path: &Path) -> bool {
        let Ok(file) = std::fs::File::open(path) else {
            return false;
        };
        // A shared lock is refused only while someone holds the exclusive one.
        flock_retrying(&file, FlockOperation::NonBlockingLockShared).is_ok_and(|taken| !taken)
    }

    fn append(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;
        file.write_all(contents)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_dir(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<OsString>> {
        let mut names = std::fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<io::Result<Vec<_>>>()?;
        names.sort_by(|a, b| a.as_encoded_bytes().cmp(b.as_encoded_bytes()));
        Ok(names)
    }

    fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        let metadata = std::fs::metadata(path)?;
        let kind = if metadata.is_file() {
            FileKind::File
        } else if metadata.is_dir() {
            FileKind::Directory
        } else if metadata.file_type().is_block_device() {
            FileKind::BlockDevice
        } else if metadata.file_type().is_char_device() {
            FileKind::CharDevice
        } else if metadata.file_type().is_socket() {
            FileKind::Socket
        } else {
            FileKind::Other
        };
        Ok(Metadata {
            kind,
            mode: metadata.mode() & 0o7777,
            len: metadata.len(),
            modified: metadata.modified()?,
        })
    }

    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

#[cfg(test)]
mod tests;
