//! `mount`, `umount` and `mountpoint`.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::path::Path;

use zbm_platform::{Command, Input, Output, Runner, Stream};

use crate::Shown;

/// The mount points of the system, run through a [`Runner`].
#[derive(Clone, Copy)]
pub struct Mount<'a> {
    runner: &'a dyn Runner,
}

/// `command` with its output going where `shown` says.
fn shown(command: Command, shown: Shown) -> Command {
    match shown {
        Shown::Console => command.stdout(Stream::Inherit),
        Shown::Hidden => command
            .stdin(Input::Null)
            .stdout(Stream::Null)
            .stderr(Stream::Null),
    }
}

impl<'a> Mount<'a> {
    /// The programs, run through `runner`.
    pub fn new(runner: &'a dyn Runner) -> Mount<'a> {
        Mount { runner }
    }

    /// Mounts the ZFS filesystem or snapshot `fs` on `target` with `options`
    /// (`mount -o OPTIONS -t zfs FS TARGET`); its error messages go to the
    /// console unless `errors` is [`Shown::Hidden`].
    ///
    /// # Errors
    ///
    /// The error that kept `mount` from starting.
    pub fn zfs(&self, fs: &str, target: &Path, options: &str, errors: Shown) -> io::Result<Output> {
        let stderr = match errors {
            Shown::Console => Stream::Inherit,
            Shown::Hidden => Stream::Null,
        };
        let command = Command::new("mount")
            .args(["-o", options, "-t", "zfs", fs])
            .arg(target)
            .stderr(stderr);
        self.runner.run(&command)
    }

    /// Mounts `device` read-only on `dir`, quietly (`mount -o ro DEVICE DIR`).
    ///
    /// # Errors
    ///
    /// The error that kept `mount` from starting.
    pub fn read_only(&self, device: &str, dir: &str) -> io::Result<Output> {
        let command = Command::new("mount").args(["-o", "ro", device, dir]);
        self.runner.run(&shown(command, Shown::Hidden))
    }

    /// Mounts `source` read-only on `dir` with `mount -r SOURCE DIR`, its
    /// output on the console.
    ///
    /// # Errors
    ///
    /// The error that kept `mount` from starting.
    pub fn read_only_shown(&self, source: &str, dir: &Path) -> io::Result<Output> {
        let command = Command::new("mount").arg("-r").arg(source).arg(dir);
        self.runner.run(&shown(command, Shown::Console))
    }

    /// Mounts, or with `options` `remount,...` remounts, `efivarfs` on
    /// `location` (`mount -t efivarfs efivarfs LOCATION -o OPTIONS`).
    ///
    /// # Errors
    ///
    /// The error that kept `mount` from starting.
    pub fn efivarfs(&self, location: &str, options: &str) -> io::Result<Output> {
        let command =
            Command::new("mount").args(["-t", "efivarfs", "efivarfs", location, "-o", options]);
        self.runner.run(&shown(command, Shown::Console))
    }

    /// Whether `path` is a mount point (`mountpoint -q PATH`).
    pub fn is_mountpoint(&self, path: &str) -> bool {
        let command = Command::new("mountpoint").args(["-q", path]);
        self.runner
            .run(&shown(command, Shown::Hidden).stdin(Input::Inherit))
            .is_ok_and(|output| output.status.is_success())
    }

    /// Unmounts `path` (`umount PATH`).
    ///
    /// # Errors
    ///
    /// The error that kept `umount` from starting.
    pub fn unmount(&self, path: &Path, output: Shown) -> io::Result<Output> {
        let command = Command::new("umount").arg(path);
        self.runner.run(&shown(command, output))
    }

    /// Unmounts `path` and everything below it, quietly (`umount -R PATH`).
    ///
    /// # Errors
    ///
    /// The error that kept `umount` from starting.
    pub fn unmount_recursive(&self, path: &Path) -> io::Result<Output> {
        let command = Command::new("umount").arg("-R").arg(path);
        self.runner.run(&shown(command, Shown::Hidden))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;

    #[test]
    fn command_lines_and_streams_are_those_of_the_legacy_code() {
        let runner = FakeRunner::new();
        runner.respond("mountpoint -q /sys/firmware/efi/efivars", 1, "");
        let mount = Mount::new(&runner);
        let mnt = Path::new("/zfsbootmenu/mnt");

        mount
            .zfs("tank/ROOT/void", mnt, "zfsutil,ro", Shown::Hidden)
            .unwrap();
        mount
            .zfs("tank/ROOT/void@a", mnt, "ro", Shown::Console)
            .unwrap();
        mount.read_only("/dev/sdb1", "/tmp/luks-key.1-0").unwrap();
        mount
            .read_only_shown("tank/hooks", Path::new("/.external_hooks"))
            .unwrap();
        mount
            .efivarfs("/sys/firmware/efi/efivars", "remount,rw")
            .unwrap();
        assert!(!mount.is_mountpoint("/sys/firmware/efi/efivars"));
        mount.unmount(mnt, Shown::Console).unwrap();
        mount.unmount(mnt, Shown::Hidden).unwrap();
        mount.unmount_recursive(mnt).unwrap();

        assert_eq!(
            runner.lines(),
            [
                "mount -o zfsutil,ro -t zfs tank/ROOT/void /zfsbootmenu/mnt",
                "mount -o ro -t zfs tank/ROOT/void@a /zfsbootmenu/mnt",
                "mount -o ro /dev/sdb1 /tmp/luks-key.1-0",
                "mount -r tank/hooks /.external_hooks",
                "mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o remount,rw",
                "mountpoint -q /sys/firmware/efi/efivars",
                "umount /zfsbootmenu/mnt",
                "umount /zfsbootmenu/mnt",
                "umount -R /zfsbootmenu/mnt",
            ]
        );
        let calls = runner.calls();
        assert_eq!(calls[0].get_stderr(), Stream::Null);
        assert_eq!(calls[1].get_stderr(), Stream::Inherit);
        assert_eq!(calls[2].get_stdout(), Stream::Null);
        assert_eq!(calls[3].get_stdout(), Stream::Inherit);
        assert_eq!(calls[6].get_stdout(), Stream::Inherit);
        assert_eq!(*calls[7].get_stdin(), Input::Null);
    }
}
