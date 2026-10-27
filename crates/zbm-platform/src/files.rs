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
mod tests {
    use super::*;
    use zbm_testkit::TempDir;

    #[test]
    fn real_files_follow_the_shell_commands() {
        let dir = TempDir::new("zbm-files");
        let files = RealFiles;
        let file = dir.join("state/bootfs");
        assert!(files.write(&file, b"x").is_err());
        files.create_dir_all(&dir.join("state")).unwrap();
        files.write(&file, b"tank/ROOT/one\n").unwrap();
        files.append(&file, b"tank/ROOT/two\n").unwrap();
        assert_eq!(
            files.read_to_string(&file).unwrap(),
            "tank/ROOT/one\ntank/ROOT/two\n"
        );
        assert!(files.exists(&file) && files.is_file(&file) && !files.is_dir(&file));
        assert!(files.is_dir(&dir.join("state")));

        let metadata = files.metadata(&file).unwrap();
        assert_eq!(metadata.kind, FileKind::File);
        assert_eq!(metadata.len, 28);
        assert!(!metadata.is_executable());
        files.set_mode(&file, 0o755).unwrap();
        let metadata = files.metadata(&file).unwrap();
        assert_eq!(metadata.mode, 0o755);
        assert!(metadata.is_executable());
        assert_eq!(
            files.metadata(Path::new("/dev/null")).unwrap().kind,
            FileKind::CharDevice
        );
        let random = files.read_prefix(Path::new("/dev/urandom"), 32).unwrap();
        assert_eq!(random.len(), 32);
        assert_eq!(files.read_prefix(&file, 4).unwrap(), b"tank");

        let lock = dir.join("state/lock");
        files.create_dir(&lock).unwrap();
        assert_eq!(
            files.create_dir(&lock).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        files.remove_dir(&lock).unwrap();
        assert!(!files.exists(&lock));

        files.copy(&file, &dir.join("state/copy")).unwrap();
        files
            .rename(&dir.join("state/copy"), &dir.join("state/moved"))
            .unwrap();
        files.remove_file(&dir.join("state/moved")).unwrap();
        assert!(files.remove_file(&dir.join("state/moved")).is_err());
    }

    #[test]
    fn directories_list_in_byte_order() {
        let dir = TempDir::new("zbm-files");
        for name in ["b", "B", "a.completed", "10-x", "9-y"] {
            dir.write(name, "");
        }
        let names = RealFiles.read_dir(dir.path()).unwrap();
        assert_eq!(names, ["10-x", "9-y", "B", "a.completed", "b"]);
    }

    #[test]
    fn replacing_leaves_no_temporary_file() {
        let dir = TempDir::new("zbm-files");
        let file = dir.join("kernels");
        RealFiles.write(&file, b"old").unwrap();
        RealFiles.replace(&file, b"new").unwrap();
        assert_eq!(RealFiles.read(&file).unwrap(), b"new");
        assert_eq!(RealFiles.read_dir(dir.path()).unwrap(), ["kernels"]);
        assert!(RealFiles.replace(Path::new("/"), b"").is_err());
        assert!(RealFiles.replace(&dir.join("missing/file"), b"").is_err());

        // A rename that fails takes its temporary file with it.
        dir.write("full/entry", "");
        assert!(RealFiles.replace(&dir.join("full"), b"x").is_err());
        assert_eq!(RealFiles.read_dir(dir.path()).unwrap(), ["full", "kernels"]);
    }

    #[test]
    fn private_files_are_new_mode_0600_and_never_through_a_link() {
        let dir = TempDir::new("zbm-files");
        let key = dir.join("key");
        RealFiles.write_private(&key, b"secret").unwrap();
        assert_eq!(RealFiles.read(&key).unwrap(), b"secret");
        assert_eq!(RealFiles.metadata(&key).unwrap().mode, 0o600);
        let again = RealFiles.write_private(&key, b"other").unwrap_err();
        assert_eq!(again.kind(), io::ErrorKind::AlreadyExists);

        dir.write("target", "kept");
        std::os::unix::fs::symlink(dir.join("target"), dir.join("link")).unwrap();
        std::os::unix::fs::symlink(dir.join("nowhere"), dir.join("dangling")).unwrap();
        for link in ["link", "dangling"] {
            let error = RealFiles.write_private(&dir.join(link), b"x").unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{link}");
        }
        assert_eq!(RealFiles.read(&dir.join("target")).unwrap(), b"kept");
        assert!(!dir.join("nowhere").exists());
    }

    #[test]
    fn special_files_have_their_kinds() {
        let dir = TempDir::new("zbm-files");
        let kind = |path: &Path| RealFiles.metadata(path).unwrap().kind;
        let _socket = std::os::unix::net::UnixListener::bind(dir.join("socket")).unwrap();
        let fifo = std::process::Command::new("mkfifo")
            .arg(dir.join("fifo"))
            .status()
            .unwrap();
        assert!(fifo.success());

        assert_eq!(kind(&dir.join("socket")), FileKind::Socket);
        assert_eq!(kind(&dir.join("fifo")), FileKind::Other);
        assert_eq!(kind(Path::new("/dev/null")), FileKind::CharDevice);
        let block = std::fs::read_dir("/dev")
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.file_type().is_ok_and(|kind| kind.is_block_device()));
        if let Some(block) = block {
            assert_eq!(kind(&block.path()), FileKind::BlockDevice);
        }
    }

    #[test]
    fn canonical_paths_have_no_links() {
        let dir = TempDir::new("zbm-files");
        dir.write("target", "");
        std::os::unix::fs::symlink(dir.join("target"), dir.join("link")).unwrap();
        let canonical = RealFiles.canonicalize(&dir.join("./link")).unwrap();
        assert_eq!(
            canonical,
            RealFiles.canonicalize(&dir.join("target")).unwrap()
        );
    }
}
