//! Key files: finding a volume's key file, on a key device when it names
//! one, and releasing the key device afterwards.
//!
//! A key file is a path in the image, or `path:device` for a file on another
//! device, such as a USB stick, which is mounted read-only for as long as the
//! key is needed.

use std::path::Path;

use zbm_platform::files::FileKind;
use zbm_tools::Shown;
use zbm_tools::mount::Mount;
use zbm_types::DevicePath;

use crate::device::{device_present, resolve_device};
use crate::unlocker::Unlocker;
use zbm_platform::interaction::Wait;
use zbm_platform::interaction::{Awaited, VolumeRef, WaitRequest};

/// What looking for a key file found.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FoundKey {
    /// The key file, if it can be read.
    pub path: Option<String>,
    /// The mount point of the key device, to release afterwards.
    pub mount: Option<String>,
}

/// Splits `path:device` into the path and the device specification; a path
/// alone has none.
pub fn split_keyfile(spec: &str) -> (&str, Option<&str>) {
    match spec.split_once(':') {
        Some((path, device)) if spec.starts_with('/') => (path, Some(device)),
        _ => (spec, None),
    }
}

impl Unlocker<'_> {
    /// Finds the key file of a key file specification. A missing key device
    /// is waited for once per run, as security keys are, but not when the
    /// import loop retries.
    pub fn find_keyfile(&mut self, spec: &str, volume: VolumeRef<'_>) -> FoundKey {
        if spec.is_empty() {
            return FoundKey::default();
        }
        let (file, device_spec) = split_keyfile(spec);
        let Some(device_spec) = device_spec.filter(|spec| !spec.is_empty()) else {
            let mut found = FoundKey::default();
            if self.system().files.exists(Path::new(file)) {
                found.path = self.readable(file);
            } else {
                zbm_trace::warn!("LUKS key file {file} not found");
            }
            return found;
        };
        match self.key_device(device_spec, file, volume) {
            Some(device) => self.mount_key_device(device.as_str(), file),
            None => FoundKey::default(),
        }
    }

    /// The device of the key device specification `device_spec`, waiting
    /// for it once per run (USB storage often appears a few seconds after
    /// boot) for the key file of `volume`; `None`, remembered, when it does
    /// not come.
    fn key_device(
        &mut self,
        device_spec: &str,
        file: &str,
        volume: VolumeRef<'_>,
    ) -> Option<DevicePath> {
        let system = *self.system();
        if let Some(device) = resolve_device(&system, device_spec) {
            return Some(device);
        }
        let mut device = None;
        if !self.settings().retry && !self.keydev_absent.contains(device_spec) {
            let message = self.paint(
                "cyan",
                &format!(
                    "Insert the key device {} holding {file}",
                    self.paint("yellow", device_spec)
                ),
            );
            let request = WaitRequest {
                volume,
                awaited: Awaited::KeyDevice {
                    spec: device_spec.to_owned(),
                    file: file.to_owned(),
                },
                seconds: self.token_wait(),
                present: &|| device_present(&system, device_spec),
            };
            let waited = self.await_device(&request, &message);
            if waited == Wait::Present {
                device = resolve_device(&system, device_spec);
            }
        }
        if device.is_none() {
            self.keydev_absent.insert(device_spec.to_owned());
            zbm_trace::warn!("LUKS key device {device_spec} not found");
        }
        device
    }

    /// Mounts the key device `device` read-only and finds `file` on it.
    fn mount_key_device(&self, device: &str, file: &str) -> FoundKey {
        let mut found = FoundKey::default();
        let files = self.system().files;
        let _ = files.create_dir_all(Path::new("/tmp"));
        let Some(dir) = self.temporary_dir("/tmp/luks-key") else {
            zbm_trace::error!("unable to create a mount point for key device {device}");
            return found;
        };
        let mounted = Mount::new(self.system().runner)
            .read_only(device, &dir)
            .is_ok_and(|output| output.status.is_success());
        if !mounted {
            zbm_trace::warn!("unable to mount LUKS key device {device}");
            let _ = files.remove_dir(Path::new(&dir));
            return found;
        }
        let path = format!("{dir}{file}");
        found.mount = Some(dir);
        if files.exists(Path::new(&path)) {
            found.path = self.readable(&path);
        } else {
            zbm_trace::warn!("LUKS key file {file} not found on {device}");
        }
        found
    }

    /// `path`, unless it is something a read of can wait on for ever, such
    /// as a named pipe: a key file is a regular file, or a block device
    /// holding a raw key.
    fn readable(&self, path: &str) -> Option<String> {
        let kind = self.system().files.metadata(Path::new(path)).ok()?.kind;
        if matches!(kind, FileKind::File | FileKind::BlockDevice) {
            return Some(path.to_owned());
        }
        zbm_trace::warn!("LUKS key file {path} is not a regular file or a block device, skipped");
        None
    }

    /// Unmounts and removes a key device's mount point.
    pub fn release_keyfile(&self, found: &FoundKey) {
        let Some(mount) = &found.mount else {
            return;
        };
        let _ = Mount::new(self.system().runner).unmount(Path::new(mount), Shown::Hidden);
        let _ = self.system().files.remove_dir(Path::new(mount));
    }

    /// Creates a new directory `<prefix>.<process ID>-<n>`, as
    /// `mktemp -d <prefix>.XXXXXX` creates one: a name that exists already is
    /// never reused.
    pub(crate) fn temporary_dir(&self, prefix: &str) -> Option<String> {
        let system = self.system();
        let pid = system.processes.id();
        for attempt in 0..100 {
            let dir = format!("{prefix}.{pid}-{attempt}");
            match system.files.create_dir(Path::new(&dir)) {
                Ok(()) => return Some(dir),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return None,
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{NoPrompts, VOLUME, log};
    use crate::unlocker::{Luks, Settings};
    use zbm_platform::exec::{Output, Status};
    use zbm_testkit::fakes::{ConsoleInput, FakeSystem};

    fn with_session<T>(
        fake: &FakeSystem,
        retry: bool,
        body: impl FnOnce(&mut Unlocker<'_>) -> T,
    ) -> T {
        let mut settings = Settings::new("/zfsbootmenu");
        settings.retry = retry;
        let _log = log(fake);
        let luks = Luks {
            system: fake.system(),
            settings: &settings,
            interaction: &NoPrompts,
        };
        body(&mut Unlocker::new(luks))
    }

    #[test]
    fn temporary_directories_are_never_reused() {
        let fake = FakeSystem::new();
        let pid = fake.system().processes.id();
        for attempt in 0..100 {
            fake.files.dir(format!("/tmp/x.{pid}-{attempt}"));
        }
        assert_eq!(
            with_session(&fake, false, |s| s.temporary_dir("/tmp/x")),
            None
        );
        fake.files.fail(
            format!("/tmp/y.{pid}-0"),
            std::io::ErrorKind::PermissionDenied,
        );
        assert_eq!(
            with_session(&fake, false, |s| s.temporary_dir("/tmp/y")),
            None
        );
        assert_eq!(
            with_session(&fake, false, |s| s.temporary_dir("/tmp/z")),
            Some(format!("/tmp/z.{pid}-0"))
        );
    }

    #[test]
    fn specifications_split_at_the_first_colon_of_a_path() {
        assert_eq!(split_keyfile("/k"), ("/k", None));
        assert_eq!(split_keyfile("/k:LABEL=a:b"), ("/k", Some("LABEL=a:b")));
        assert_eq!(split_keyfile("/k:"), ("/k", Some("")));
        assert_eq!(split_keyfile("k:LABEL=x"), ("k:LABEL=x", None));
    }

    #[test]
    fn plain_key_files_are_used_where_they_are() {
        let fake = FakeSystem::new();
        fake.files.file("/etc/k", "key").dir("/etc/dir.key");
        with_session(&fake, false, |session| {
            assert_eq!(
                session.find_keyfile("/etc/k", VOLUME).path.as_deref(),
                Some("/etc/k")
            );
            // A directory passed `[ -r ]` and failed in cryptsetup; it is
            // skipped before.
            assert!(session.find_keyfile("/etc/dir.key", VOLUME).path.is_none());
            assert_eq!(
                session.find_keyfile("/missing", VOLUME),
                FoundKey::default()
            );
            assert_eq!(
                session.find_keyfile("/missing:", VOLUME),
                FoundKey::default()
            );
            assert_eq!(session.find_keyfile("", VOLUME), FoundKey::default());
        });
        assert_eq!(
            crate::testing::warnings(&fake),
            [
                "LUKS key file /etc/dir.key is not a regular file or a block device, skipped",
                "LUKS key file /missing not found",
                "LUKS key file /missing not found"
            ]
        );
        assert!(fake.runner.calls().is_empty());
    }

    #[test]
    fn a_key_file_that_could_block_is_skipped() {
        let fake = FakeSystem::new();
        fake.files
            .fifo("/keys/pipe.key")
            .block_device("/dev/sdc")
            .file("/keys/a.key", "key");
        with_session(&fake, false, |session| {
            assert_eq!(
                session.find_keyfile("/keys/pipe.key", VOLUME),
                FoundKey::default()
            );
            let raw = session.find_keyfile("/dev/sdc", VOLUME);
            assert_eq!(raw.path.as_deref(), Some("/dev/sdc"));
            let file = session.find_keyfile("/keys/a.key", VOLUME);
            assert_eq!(file.path.as_deref(), Some("/keys/a.key"));
        });
        assert!(
            crate::testing::warnings(&fake)
                .iter()
                .any(|line| line.starts_with("LUKS key file /keys/pipe.key is not a regular file"))
        );
    }

    #[test]
    fn key_devices_are_mounted_read_only() {
        let fake = FakeSystem::new();
        fake.files
            .block_device("/dev/sdb1")
            .symlink("/dev/disk/by-label/KEYS", "../../sdb1");
        // The stand-in for mount puts the key file in the mount point.
        let files = fake.files.clone();
        fake.runner.on(move |command| {
            let dir = command.get_args().last()?.to_string_lossy().into_owned();
            (command.program() == "mount" && dir.ends_with("-0")).then(|| {
                files.file(format!("{dir}/keys/a.key"), "key");
                Ok(Output::new(Status::exited(0), ""))
            })
        });
        fake.runner.respond_program("mount", 0, "");
        let found = with_session(&fake, false, |session| {
            let found = session.find_keyfile("/keys/a.key:LABEL=KEYS", VOLUME);
            let missing = session.find_keyfile("/keys/b.key:LABEL=KEYS", VOLUME);
            assert!(missing.path.is_none() && missing.mount.is_some());
            session.release_keyfile(&missing);
            session.release_keyfile(&found);
            found
        });
        let mount = found.mount.unwrap();
        assert!(mount.starts_with("/tmp/luks-key."));
        assert_eq!(found.path, Some(format!("{mount}/keys/a.key")));
        let lines = fake.runner.lines();
        assert_eq!(lines[0], format!("mount -o ro /dev/sdb1 {mount}"));
        assert_eq!(lines[3], format!("umount {mount}"));
        assert!(
            crate::testing::warnings(&fake)
                .iter()
                .any(|line| line.starts_with("LUKS key file /keys/b.key not found on /dev/sdb1"))
        );
    }

    #[test]
    fn missing_key_devices_are_waited_for_once() {
        let fake = FakeSystem::new();
        fake.files
            .file("/zfsbootmenu/cmdline.d/zbm.luks_token_wait", "2");
        fake.console.timeout().timeout();
        with_session(&fake, false, |session| {
            assert_eq!(
                session.find_keyfile("/k:LABEL=NONE", VOLUME),
                FoundKey::default()
            );
            // The second volume on the same stick does not wait again.
            assert_eq!(
                session.find_keyfile("/l:LABEL=NONE", VOLUME),
                FoundKey::default()
            );
            session.forget_secrets();
            fake.console.push(ConsoleInput::Key(b'x'));
            assert_eq!(
                session.find_keyfile("/k:LABEL=NONE", VOLUME),
                FoundKey::default()
            );
        });
        let output = fake.console.output_text();
        assert_eq!(output.matches("Insert the key device").count(), 2);
        assert!(output.contains("Waiting 2 seconds; press any key to skip."));
        // Import retries never wait.
        let fake = FakeSystem::new();
        with_session(&fake, true, |session| {
            assert_eq!(
                session.find_keyfile("/k:LABEL=NONE", VOLUME),
                FoundKey::default()
            );
        });
        assert_eq!(fake.console.output_text(), "");
        assert_eq!(
            crate::testing::warnings(&fake),
            ["LUKS key device LABEL=NONE not found"]
        );
    }

    #[test]
    fn failed_mounts_remove_the_mount_point() {
        let fake = FakeSystem::new();
        fake.files.block_device("/dev/sdb1");
        fake.runner.respond_program("mount", 32, "");
        with_session(&fake, false, |session| {
            assert_eq!(
                session.find_keyfile("/k:/dev/sdb1", VOLUME),
                FoundKey::default()
            );
        });
        assert!(
            !fake
                .files
                .paths()
                .iter()
                .any(|path| path.starts_with("/tmp/luks-key."))
        );
        assert_eq!(
            crate::testing::warnings(&fake),
            ["unable to mount LUKS key device /dev/sdb1"]
        );
    }
}
