//! File tests as Perl and the shell perform them.
//!
//! Perl's `-f`, `-d`, `-r` and `-x` and the shell's `[ -f ]`, `[ -r ]` and `[ -x ]` follow symbolic links and judge
//! access by the effective user: root may read any file, and may execute any file with an execute bit set, and any
//! directory. The functions here do the same, so the Rust tools accept and reject the same files.

use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

use zbm_sys::Credentials;

/// Whether `path` is a regular file, following symbolic links (`-f`).
pub fn is_file(path: impl AsRef<Path>) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

/// Whether `path` is a directory, following symbolic links (`-d`).
pub fn is_dir(path: impl AsRef<Path>) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

/// Whether `path` exists, following symbolic links (`-e`).
pub fn exists(path: impl AsRef<Path>) -> bool {
    std::fs::metadata(path).is_ok()
}

/// The kind of access to test.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    /// Read access (`-r`).
    Read,
    /// Write access (`-w`).
    Write,
    /// Execute access (`-x`).
    Execute,
}

impl Access {
    /// The owner permission bit of the access; the group and other bits are 3 and 6 bits lower.
    fn owner_bit(self) -> u32 {
        match self {
            Access::Read => 0o400,
            Access::Write => 0o200,
            Access::Execute => 0o100,
        }
    }
}

/// Whether the process may access `path` the given way (`-r`, `-w`, `-x`).
pub fn can_access(path: impl AsRef<Path>, access: Access) -> bool {
    can_access_as(path, access, &zbm_sys::credentials())
}

/// Whether a process with the given credentials may access `path` the given way.
pub fn can_access_as(path: impl AsRef<Path>, access: Access, credentials: &Credentials) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    let file = FileAccess {
        mode: metadata.permissions().mode(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        is_dir: metadata.is_dir(),
    };
    file.permits(access, credentials)
}

/// The parts of a file's metadata that decide access.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileAccess {
    /// The permission bits.
    pub mode: u32,
    /// The owner.
    pub uid: u32,
    /// The group.
    pub gid: u32,
    /// Whether the file is a directory.
    pub is_dir: bool,
}

impl FileAccess {
    /// Whether a process with the given credentials may access the file the given way.
    ///
    /// This is Perl's `cando`: root reads and writes anything and executes anything with an execute bit, or any
    /// directory; other users get the owner, group or other bits, whichever class they belong to first.
    pub fn permits(self, access: Access, credentials: &Credentials) -> bool {
        let bit = access.owner_bit();
        if credentials.uid == 0 {
            return match access {
                Access::Read | Access::Write => true,
                Access::Execute => self.mode & 0o111 != 0 || self.is_dir,
            };
        }
        if self.uid == credentials.uid {
            self.mode & bit != 0
        } else if self.gid == credentials.gid || credentials.groups.contains(&self.gid) {
            self.mode & (bit >> 3) != 0
        } else {
            self.mode & (bit >> 6) != 0
        }
    }
}

/// Whether the process may read `path` (`-r`).
pub fn is_readable(path: impl AsRef<Path>) -> bool {
    can_access(path, Access::Read)
}

/// Whether the process may write `path` (`-w`).
pub fn is_writable(path: impl AsRef<Path>) -> bool {
    can_access(path, Access::Write)
}

/// Whether the process may execute `path` (`-x`).
pub fn is_executable(path: impl AsRef<Path>) -> bool {
    can_access(path, Access::Execute)
}

/// Creates a new, empty file readable and writable only by its owner, named after `template` in `dir`, as
/// `mktemp` does: the trailing `X` characters of the template are replaced by random letters and digits.
pub fn create_temp_file(dir: &Path, template: &str) -> std::io::Result<std::path::PathBuf> {
    create_temp(dir, template, |path| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map(drop)
    })
}

/// Creates a new directory accessible only by its owner, named after `template` in `dir`, as `mktemp -d` does.
pub fn create_temp_dir(dir: &Path, template: &str) -> std::io::Result<std::path::PathBuf> {
    create_temp(dir, template, |path| {
        std::fs::DirBuilder::new().mode(0o700).create(path)
    })
}

fn create_temp(
    dir: &Path,
    template: &str,
    create: impl Fn(&Path) -> std::io::Result<()>,
) -> std::io::Result<std::path::PathBuf> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let stem = template.trim_end_matches('X');
    let random_len = template.len() - stem.len();
    let mut seed = random_seed();
    for _ in 0..100 {
        let suffix: String = (0..random_len)
            .map(|_| {
                // xorshift64
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let index = usize::try_from(seed % CHARS.len() as u64).unwrap_or_default();
                char::from(CHARS[index])
            })
            .collect();
        let path = dir.join(format!("{stem}{suffix}"));
        match create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "no unused temporary name found",
    ))
}

/// A seed for temporary names, from the kernel's random source when it can be read.
fn random_seed() -> u64 {
    use std::io::Read;
    let mut bytes = [0_u8; 8];
    let from_kernel = std::fs::File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .is_ok();
    if from_kernel {
        return u64::from_ne_bytes(bytes) | 1;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| u64::from(time.subsec_nanos()));
    ((u64::from(std::process::id()) << 32) | nanos) | 1
}

/// Why [`copy_like_cp`] failed.
#[derive(Debug)]
pub enum CopyError {
    /// The source could not be read.
    Source(std::path::PathBuf, std::io::Error),
    /// The target could not be created or opened.
    Target(std::path::PathBuf, std::io::Error),
    /// Writing the target failed.
    Write(std::path::PathBuf, std::io::Error),
}

impl CopyError {
    /// The message GNU `cp` prints for the failure, in the C locale.
    pub fn cp_message(&self) -> String {
        match self {
            CopyError::Source(path, error) => {
                format!("cp: cannot stat '{}': {}", path.display(), strerror(error))
            }
            CopyError::Target(path, error) => format!(
                "cp: cannot create regular file '{}': {}",
                path.display(),
                strerror(error)
            ),
            CopyError::Write(path, error) => {
                format!(
                    "cp: error writing '{}': {}",
                    path.display(),
                    strerror(error)
                )
            }
        }
    }
}

/// The C library's description of an error, without the `(os error N)` that Rust adds.
pub fn strerror(error: &std::io::Error) -> String {
    let text = error.to_string();
    match text.rfind(" (os error ") {
        Some(end) => text[..end].to_owned(),
        None => text,
    }
}

/// Copies a file as `cp source target` (without `-p`) does: an existing target keeps its permissions and is
/// overwritten, a new one gets the permissions of the source less the umask, and a target that is a directory
/// receives a file with the source's name.
pub fn copy_like_cp(source: &Path, target: &Path) -> Result<(), CopyError> {
    let target = if is_dir(target) {
        target.join(source.file_name().unwrap_or_default())
    } else {
        target.to_owned()
    };
    let source_error = |error| CopyError::Source(source.to_owned(), error);
    let mode = std::fs::metadata(source)
        .map_err(source_error)?
        .permissions()
        .mode()
        & 0o7777;
    let mut input = std::fs::File::open(source).map_err(source_error)?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(&target)
        .map_err(|error| CopyError::Target(target.clone(), error))?;
    std::io::copy(&mut input, &mut output)
        .map_err(|error| CopyError::Write(target.clone(), error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let path =
                std::env::temp_dir().join(format!("zbm-core-fs-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }

        fn file(&self, name: &str, mode: u32) -> std::path::PathBuf {
            let path = self.0.join(name);
            fs::write(&path, b"x").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn user(uid: u32, gid: u32, groups: &[u32]) -> Credentials {
        Credentials {
            uid,
            gid,
            groups: groups.to_vec(),
        }
    }

    #[test]
    fn file_and_directory_tests_follow_links() {
        let scratch = Scratch::new("kinds");
        let file = scratch.file("file", 0o644);
        let link = scratch.0.join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let dangling = scratch.0.join("dangling");
        std::os::unix::fs::symlink(scratch.0.join("missing"), &dangling).unwrap();

        assert!(is_file(&file));
        assert!(is_file(&link));
        assert!(!is_file(&dangling));
        assert!(!exists(&dangling));
        assert!(is_dir(&scratch.0));
        assert!(!is_file(&scratch.0));
        assert!(!is_dir(&file));
    }

    fn file(mode: u32) -> FileAccess {
        FileAccess {
            mode,
            uid: 1000,
            gid: 100,
            is_dir: false,
        }
    }

    #[test]
    fn root_reads_anything_and_executes_with_any_x_bit() {
        let root = user(0, 0, &[]);
        assert!(file(0o000).permits(Access::Read, &root));
        assert!(!file(0o000).permits(Access::Execute, &root));
        assert!(file(0o001).permits(Access::Execute, &root));
        assert!(file(0o010).permits(Access::Execute, &root));
        let directory = FileAccess {
            is_dir: true,
            ..file(0o000)
        };
        assert!(directory.permits(Access::Execute, &root));
    }

    #[test]
    fn other_users_get_the_first_matching_class() {
        // The owner class decides for the owner, even though the group and others may read.
        assert!(!file(0o077).permits(Access::Read, &user(1000, 100, &[])));
        assert!(file(0o500).permits(Access::Execute, &user(1000, 5, &[])));
        // Members of the group, by primary or supplementary ID, get the group bits.
        assert!(file(0o050).permits(Access::Read, &user(2000, 100, &[])));
        assert!(file(0o010).permits(Access::Execute, &user(2000, 5, &[7, 100])));
        assert!(!file(0o007).permits(Access::Read, &user(2000, 100, &[])));
        // Everybody else gets the other bits.
        assert!(file(0o004).permits(Access::Read, &user(2000, 5, &[])));
        assert!(!file(0o004).permits(Access::Execute, &user(2000, 5, &[])));
        assert!(!file(0o770).permits(Access::Read, &user(2000, 5, &[])));
    }

    #[test]
    fn temporary_files_are_private_and_unique() {
        let scratch = Scratch::new("temp");
        let first = create_temp_file(&scratch.0, "tmp.XXXXXXXXXX").unwrap();
        let second = create_temp_file(&scratch.0, "tmp.XXXXXXXXXX").unwrap();
        assert_ne!(first, second);
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("tmp.") && name.len() == 14, "{name}");
        assert_eq!(
            fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let dir = create_temp_dir(&scratch.0, "XXXXXXXXXX").unwrap();
        assert!(is_dir(&dir));
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn copies_like_cp() {
        let scratch = Scratch::new("copy");
        let source = scratch.file("source", 0o600);
        fs::write(&source, b"new contents").unwrap();
        // An existing target keeps its mode.
        let existing = scratch.file("existing", 0o644);
        copy_like_cp(&source, &existing).unwrap();
        assert_eq!(fs::read(&existing).unwrap(), b"new contents");
        assert_eq!(
            fs::metadata(&existing).unwrap().permissions().mode() & 0o777,
            0o644
        );
        // A new target takes the source's mode.
        let created = scratch.0.join("created");
        copy_like_cp(&source, &created).unwrap();
        assert_eq!(
            fs::metadata(&created).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let error = copy_like_cp(&source, &scratch.0.join("missing/target")).unwrap_err();
        assert_eq!(
            error.cp_message(),
            format!(
                "cp: cannot create regular file '{}': No such file or directory",
                scratch.0.join("missing/target").display()
            )
        );
        let error = copy_like_cp(&scratch.0.join("nothing"), &created).unwrap_err();
        assert!(error.cp_message().starts_with("cp: cannot stat '"));
        // A directory receives a file of the same name.
        let dir = scratch.0.join("dir");
        fs::create_dir(&dir).unwrap();
        copy_like_cp(&source, &dir).unwrap();
        assert_eq!(fs::read(dir.join("source")).unwrap(), b"new contents");
    }

    #[test]
    fn missing_files_are_not_accessible() {
        let scratch = Scratch::new("missing");
        assert!(!can_access_as(
            scratch.0.join("missing"),
            Access::Read,
            &user(0, 0, &[])
        ));
        let readable = scratch.file("readable", 0o644);
        assert!(is_readable(&readable));
        assert!(!is_executable(&readable));
    }
}
