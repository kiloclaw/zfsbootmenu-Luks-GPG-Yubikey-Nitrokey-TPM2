//! What the cases see of the host's files: everything but its security keys
//! and its TPM.
//!
//! The LUKS code finds a FIDO2 key among the `hidraw` devices in `/dev`, and
//! a YubiKey's OTP interface in `/sys/class/hidraw`. A real host has its own
//! there (keyboards, mice, security keys), which would answer in place of
//! the lab's. The cases see only the lab's `/dev/hidraw99`, which has no
//! entry in `/sys/class/hidraw`, so that they find no key where the lab has
//! none, as in a container.
//!
//! The LUKS code finds a TPM as a device in `/dev`, and gives up waiting for
//! one when `/sys/class/tpm` is empty. The lab's TPM is a simulator on a
//! socket, so the cases see no TPM device at all: a case without the lab's
//! TPM finds none, and gives up at once, as in a container.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use zbm_platform::files::{FileLock, Files, LockWait, Metadata, RealFiles};

/// The lab's security key, the one `hidraw` device the cases see.
const LAB_HIDRAW: &str = "hidraw99";

/// The classes in `/sys` whose devices the cases do not see.
const HIDDEN_CLASSES: [&str; 2] = ["/sys/class/hidraw", "/sys/class/tpm"];

/// The real files, without the host's `hidraw` devices and TPMs.
#[derive(Clone, Copy, Debug, Default)]
pub struct LabFiles;

impl LabFiles {
    /// Whether `path` is one of the host's `hidraw` devices or TPMs, or its
    /// entry in `/sys/class`.
    fn is_hidden(path: &Path) -> bool {
        let in_class = |class: &str| {
            path.strip_prefix(class)
                .is_ok_and(|below| !below.as_os_str().is_empty())
        };
        if HIDDEN_CLASSES.into_iter().any(in_class) {
            return true;
        }
        path.parent() == Some(Path::new("/dev"))
            && path
                .file_name()
                .is_some_and(|name| Self::is_host_device(name.as_encoded_bytes()))
    }

    /// Whether a name in `/dev` is a `hidraw` device other than the lab's,
    /// or a TPM (`tpm0`, `tpmrm0`).
    fn is_host_device(name: &[u8]) -> bool {
        let numbered = |prefix: &[u8]| {
            name.strip_prefix(prefix)
                .is_some_and(|number| !number.is_empty() && number.iter().all(u8::is_ascii_digit))
        };
        (name.starts_with(b"hidraw") && name != LAB_HIDRAW.as_bytes())
            || numbered(b"tpm")
            || numbered(b"tpmrm")
    }

    /// `NotFound` for a hidden path, so that it seems not to exist.
    fn visible(path: &Path) -> io::Result<&Path> {
        if Self::is_hidden(path) {
            Err(io::ErrorKind::NotFound.into())
        } else {
            Ok(path)
        }
    }
}

impl Files for LabFiles {
    fn shared(&self) -> std::sync::Arc<dyn Files + Send + Sync> {
        std::sync::Arc::new(LabFiles)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        RealFiles.read(Self::visible(path)?)
    }
    fn read_prefix(&self, path: &Path, len: usize) -> io::Result<Vec<u8>> {
        RealFiles.read_prefix(Self::visible(path)?, len)
    }
    fn read_available(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        RealFiles.read_available(Self::visible(path)?, limit)
    }
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        RealFiles.write(Self::visible(path)?, contents)
    }
    fn write_private(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        RealFiles.write_private(Self::visible(path)?, contents)
    }
    fn lock(&self, path: &Path, wait: LockWait) -> io::Result<Option<FileLock>> {
        RealFiles.lock(Self::visible(path)?, wait)
    }
    fn is_locked(&self, path: &Path) -> bool {
        !Self::is_hidden(path) && RealFiles.is_locked(path)
    }
    fn append(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        RealFiles.append(Self::visible(path)?, contents)
    }
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        RealFiles.create_dir(Self::visible(path)?)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        RealFiles.create_dir_all(Self::visible(path)?)
    }
    fn mount_zfs(&self, source: &str, target: &Path, read_only: bool) -> io::Result<()> {
        RealFiles.mount_zfs(source, Self::visible(target)?, read_only)
    }
    fn unmount(&self, target: &Path) -> io::Result<()> {
        RealFiles.unmount(Self::visible(target)?)
    }
    fn terminal_size(&self, path: &Path) -> io::Result<zbm_platform::console::Size> {
        RealFiles.terminal_size(Self::visible(path)?)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        RealFiles.remove_file(Self::visible(path)?)
    }
    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        RealFiles.remove_dir(Self::visible(path)?)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        RealFiles.rename(Self::visible(from)?, Self::visible(to)?)
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<OsString>> {
        let names = RealFiles.read_dir(Self::visible(path)?)?;
        Ok(names
            .into_iter()
            .filter(|name| !Self::is_hidden(&path.join(name)))
            .collect())
    }
    fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        RealFiles.metadata(Self::visible(path)?)
    }
    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        RealFiles.set_mode(Self::visible(path)?, mode)
    }
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        RealFiles.canonicalize(Self::visible(path)?)
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        RealFiles.read_link(Self::visible(path)?)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::LabFiles;

    fn hidden(path: &str) -> bool {
        LabFiles::is_hidden(Path::new(path))
    }

    #[test]
    fn the_hosts_hidraw_devices_and_their_sysfs_entries_are_hidden() {
        assert!(hidden("/dev/hidraw0"));
        assert!(hidden("/dev/hidraw12"));
        assert!(hidden("/sys/class/hidraw/hidraw0"));
        assert!(hidden("/sys/class/hidraw/hidraw0/device/uevent"));
    }

    #[test]
    fn the_hosts_tpms_and_their_sysfs_entries_are_hidden() {
        assert!(hidden("/dev/tpm0"));
        assert!(hidden("/dev/tpmrm0"));
        assert!(hidden("/sys/class/tpm/tpm0"));
    }

    #[test]
    fn the_labs_key_the_class_directories_and_other_devices_stay_visible() {
        assert!(!hidden("/dev/hidraw99"));
        assert!(!hidden("/sys/class/hidraw"));
        assert!(!hidden("/sys/class/tpm"));
        assert!(!hidden("/dev/loop0"));
        assert!(!hidden("/dev/tpm"));
        assert!(!hidden("/dev/tpmx"));
        assert!(!hidden("/dev/no-such-tpm"));
        assert!(!hidden("/dev/mapper/hidraw0"));
        assert!(!hidden("/sys/class/block/loop0"));
    }

    #[test]
    fn a_hidden_directory_lists_nothing_and_a_hidden_device_does_not_exist() {
        use zbm_platform::files::Files;
        let files = LabFiles;
        let hidraw = files.read_dir(Path::new("/sys/class/hidraw"));
        assert!(hidraw.unwrap_or_default().is_empty());
        let tpms = files.read_dir(Path::new("/sys/class/tpm"));
        assert!(tpms.unwrap_or_default().is_empty());
        assert!(!files.exists(Path::new("/dev/tpmrm0")));
        assert!(!files.exists(Path::new("/dev/hidraw0")));
        assert!(files.exists(Path::new("/dev/null")));
    }

    #[test]
    fn mounts_and_unmounts_reach_the_system_calls_as_the_real_files_do() {
        use zbm_platform::files::Files;
        let missing = Path::new("/no-such-lab-mount-point");
        let unmounted = LabFiles.unmount(missing).unwrap_err();
        assert_ne!(unmounted.kind(), std::io::ErrorKind::Unsupported);
        let mounted = LabFiles.mount_zfs("pool/fs", missing, true).unwrap_err();
        assert_ne!(mounted.kind(), std::io::ErrorKind::Unsupported);
    }
}
