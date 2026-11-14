//! Mounting and unmounting: [`Mount`], and [`Mounted`], which unmounts
//! when dropped; the kernel's file systems a chroot takes, a read-only
//! bind ([`NotReadOnly`] when it is not), and [`HostEfivars`], which sets
//! this system's `efivarfs` back to read-only when dropped; for a chroot,
//! where a path below its root leads, never through this system's links,
//! and the first link or mount point on the way to a mount point
//! ([`inside`]); and the kernel's table of mounts, read back ([`table`]).

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use zbm_platform::{Command, Files, Input, Output, Runner, Stream};

use crate::Shown;

pub mod inside;
mod special;
pub mod table;

pub use self::special::{HostEfivars, NotReadOnly};

/// The longest mounting a block device may take, for a disk or a USB stick
/// that stopped answering. ZFS mounts have none: their time grows with the
/// pool, and a suspended pool's cannot be ended anyway.
const DEVICE_TIMEOUT: Duration = Duration::from_secs(120);

/// Mounting and unmounting. ZFS mounts and single unmounts are system calls
/// through the [`Files`] port where it makes them, the real files'
/// ([`Files::mount_zfs`], [`Files::unmount`]); where the call fails, or the
/// port makes none, as in tests, the `mount` and `umount` programs run
/// instead, with the command lines of the legacy runtime.
#[derive(Clone, Copy)]
pub struct Mount<'a> {
    runner: &'a dyn Runner,
    files: &'a dyn Files,
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

/// The exit status of `run` as the shell reports it: 126 when the program
/// could not start.
fn shell_code(run: &io::Result<Output>) -> i32 {
    run.as_ref()
        .map_or(126, |output| output.status.shell_code())
}

/// Logs why the system call `call` failed, unless the port makes none.
fn log_fallback(call: &str, target: &Path, error: &io::Error) {
    if error.kind() != io::ErrorKind::Unsupported {
        zbm_trace::debug!(
            target = %target.display(),
            error = %error,
            call,
            "the system call failed; running the program"
        );
    }
}

impl<'a> Mount<'a> {
    /// Mounting with `files`' system calls and `runner`'s programs.
    pub fn new(runner: &'a dyn Runner, files: &'a dyn Files) -> Mount<'a> {
        Mount { runner, files }
    }

    /// `target`, mounted by a program that ended with status `code`.
    fn mounted(self, run: &io::Result<Output>, target: &Path) -> Result<Mounted<'a>, i32> {
        match shell_code(run) {
            0 => Ok(Mounted::new(self, target)),
            code => Err(code),
        }
    }

    /// Mounts the ZFS filesystem or snapshot `fs` read-only on `target`:
    /// `mount(2)`, or `mount -o [zfsutil,]ro -t zfs FS TARGET`, `zfsutil`
    /// for a filesystem whose `mountpoint` is not `legacy`. The program's
    /// error messages go to the console unless `errors` is
    /// [`Shown::Hidden`].
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn zfs(
        self,
        fs: &str,
        target: &Path,
        zfsutil: bool,
        errors: Shown,
    ) -> Result<Mounted<'a>, i32> {
        self.zfs_as(fs, target, (zfsutil, "ro"), errors)
    }

    /// Mounts the ZFS filesystem `fs` read/write on `target`, as
    /// [`Mount::zfs`] mounts it read-only: `mount -o [zfsutil,]rw -t zfs FS
    /// TARGET` where `mount(2)` fails.
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn zfs_writable(
        self,
        fs: &str,
        target: &Path,
        zfsutil: bool,
        errors: Shown,
    ) -> Result<Mounted<'a>, i32> {
        self.zfs_as(fs, target, (zfsutil, "rw"), errors)
    }

    /// Mounts `fs` on `target` with `zfsutil` or not, and `access`, `ro` or
    /// `rw`.
    fn zfs_as(
        self,
        fs: &str,
        target: &Path,
        (zfsutil, access): (bool, &str),
        errors: Shown,
    ) -> Result<Mounted<'a>, i32> {
        match self.files.mount_zfs(fs, target, access == "ro") {
            Ok(()) => return Ok(Mounted::new(self, target)),
            Err(error) => log_fallback("mount", target, &error),
        }
        let stderr = match errors {
            Shown::Console => Stream::Inherit,
            Shown::Hidden => Stream::Null,
        };
        let options = if zfsutil {
            format!("zfsutil,{access}")
        } else {
            access.to_owned()
        };
        let command = Command::new("mount")
            .args(["-o", options.as_str(), "-t", "zfs", fs])
            .arg(target)
            .stderr(stderr);
        let run = self.runner.run(&command);
        self.mounted(&run, target)
    }

    /// Mounts `device` read-only on `dir`, quietly (`mount -o ro DEVICE DIR`).
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn read_only(self, device: &str, dir: &Path) -> Result<Mounted<'a>, i32> {
        let command = Command::new("mount")
            .args(["-o", "ro", device])
            .arg(dir)
            .timeout(DEVICE_TIMEOUT);
        let run = self.runner.run(&shown(command, Shown::Hidden));
        self.mounted(&run, dir)
    }

    /// Mounts `device` read/write on `dir` with `mount DEVICE DIR`, the
    /// file system's type found by `mount`, as the legacy runtime mounted a
    /// block device for the recovery shell; the read/write sibling of
    /// [`Mount::read_only`]. Its error messages go to standard error.
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn block(self, device: &str, dir: &Path) -> Result<Mounted<'a>, i32> {
        let command = Command::new("mount")
            .arg(device)
            .arg(dir)
            .stdin(Input::Null)
            .stdout(Stream::Null)
            .timeout(DEVICE_TIMEOUT);
        let run = self.runner.run(&command);
        self.mounted(&run, dir)
    }

    /// Mounts `source` read-only on `dir` with `mount -r SOURCE DIR`, its
    /// output on the console.
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn read_only_shown(self, source: &str, dir: &Path) -> Result<Mounted<'a>, i32> {
        let command = Command::new("mount")
            .arg("-r")
            .arg(source)
            .arg(dir)
            .timeout(DEVICE_TIMEOUT);
        let run = self.runner.run(&shown(command, Shown::Console));
        self.mounted(&run, dir)
    }

    /// Mounts the FAT file system on `device` read-write on `dir`, quietly
    /// (`mount -t vfat -o rw,noatime DEVICE DIR`), as for the EFI system
    /// partition.
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn fat(self, device: &str, dir: &Path) -> Result<Mounted<'a>, i32> {
        let command = Command::new("mount")
            .args(["-t", "vfat", "-o", "rw,noatime", device])
            .arg(dir)
            .timeout(DEVICE_TIMEOUT);
        let run = self.runner.run(&shown(command, Shown::Hidden));
        self.mounted(&run, dir)
    }

    /// Mounts the FAT file system on `device` read-only on `dir`, quietly
    /// (`mount -t vfat -o ro,nosuid,nodev,noexec DEVICE DIR`), as for
    /// reading the EFI system partition: nothing on it is written, and
    /// nothing on it runs.
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn fat_read_only(self, device: &str, dir: &Path) -> Result<Mounted<'a>, i32> {
        let command = Command::new("mount")
            .args(["-t", "vfat", "-o", "ro,nosuid,nodev,noexec", device])
            .arg(dir)
            .timeout(DEVICE_TIMEOUT);
        let run = self.runner.run(&shown(command, Shown::Hidden));
        self.mounted(&run, dir)
    }

    /// Mounts, or with `options` `remount,...` remounts, `efivarfs` on
    /// `location` (`mount -t efivarfs efivarfs LOCATION -o OPTIONS`); whether
    /// it succeeded. It stays mounted.
    pub fn efivarfs(self, location: &str, options: &str) -> bool {
        self.efivarfs_status(Path::new(location), options) == 0
    }

    /// The exit status of `mount -t efivarfs efivarfs LOCATION -o OPTIONS`,
    /// its output on the console; 126 when it could not start.
    fn efivarfs_status(self, location: &Path, options: &str) -> i32 {
        let command = Command::new("mount")
            .args(["-t", "efivarfs", "efivarfs"])
            .arg(location)
            .args(["-o", options]);
        shell_code(&self.runner.run(&shown(command, Shown::Console)))
    }

    /// Mounts `securityfs` read-only on `location`, where the kernel says
    /// whether it is locked down.
    ///
    /// # Errors
    ///
    /// The exit status of `mount`, which did not mount it.
    pub fn securityfs(self, location: &str) -> Result<(), i32> {
        let command = Command::new("mount").args([
            "-t",
            "securityfs",
            "securityfs",
            location,
            "-o",
            "ro,nosuid,nodev,noexec",
        ]);
        match shell_code(&self.runner.run(&shown(command, Shown::Hidden))) {
            0 => Ok(()),
            code => Err(code),
        }
    }

    /// Unmounts `path`: `umount2(2)`, or `umount PATH` with its output where
    /// `output` says; whether it succeeded.
    pub fn unmount(self, path: &Path, output: Shown) -> bool {
        match self.files.unmount(path) {
            Ok(()) => return true,
            Err(error) => log_fallback("umount", path, &error),
        }
        let command = Command::new("umount").arg(path);
        shell_code(&self.runner.run(&shown(command, output))) == 0
    }

    /// Unmounts `path` and everything below it, quietly (`umount -R PATH`);
    /// whether it succeeded.
    pub fn unmount_recursive(self, path: &Path) -> bool {
        let command = Command::new("umount").arg("-R").arg(path);
        shell_code(&self.runner.run(&shown(command, Shown::Hidden))) == 0
    }

    /// Detaches the topmost mount on `path`, with everything on and below
    /// it, busy or not, quietly (`umount -l PATH`): it leaves the namespace
    /// at once, and the file system is unmounted once nothing uses it.
    ///
    /// # Errors
    ///
    /// The exit status of `umount` when it failed, 126 when it could not
    /// start.
    pub fn unmount_lazy(self, path: &Path) -> Result<(), i32> {
        let command = Command::new("umount").arg("-l").arg(path);
        match shell_code(&self.runner.run(&shown(command, Shown::Hidden))) {
            0 => Ok(()),
            code => Err(code),
        }
    }
}

/// A file system mounted on a path, which unmounts it when dropped, so that
/// no way out of the code that mounted it leaves it mounted.
#[must_use = "dropping it unmounts the file system at once"]
pub struct Mounted<'a> {
    mount: Mount<'a>,
    path: PathBuf,
    output: Shown,
    /// Whether unmounting it unmounts everything below it too.
    recursive: bool,
    /// Whether dropping it unmounts it.
    armed: bool,
}

impl<'a> Mounted<'a> {
    /// `path`, mounted by `mount`; unmounting shows `umount`'s output.
    fn new(mount: Mount<'a>, path: &Path) -> Mounted<'a> {
        Mounted {
            mount,
            path: path.to_path_buf(),
            output: Shown::Console,
            recursive: false,
            armed: true,
        }
    }

    /// The mount point.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The same mount, which `umount` unmounts quietly.
    pub fn quietly(mut self) -> Mounted<'a> {
        self.output = Shown::Hidden;
        self
    }

    /// The same mount, which `runner` unmounts where the system call
    /// fails, such as a runner no token cancels: a mount made by a program
    /// an action may cancel is still unmounted once the action is.
    pub fn unmounted_by(mut self, runner: &'a dyn Runner) -> Mounted<'a> {
        self.mount.runner = runner;
        self
    }

    /// The same mount, which unmounting unmounts with everything mounted
    /// below it, quietly ([`Mount::unmount_recursive`]), as the root of a
    /// chroot, where the user may have mounted more.
    pub fn recursive(mut self) -> Mounted<'a> {
        self.recursive = true;
        self
    }

    /// Unmounts it now; whether that succeeded.
    pub fn unmount(mut self) -> bool {
        self.armed = false;
        if self.recursive {
            self.mount.unmount_recursive(&self.path)
        } else {
            self.mount.unmount(&self.path, self.output)
        }
    }

    /// Leaves it mounted, for code that unmounts it later, such as the
    /// recursive unmount before `kexec`; the mount point.
    pub fn leave(mut self) -> PathBuf {
        self.armed = false;
        std::mem::take(&mut self.path)
    }
}

impl Drop for Mounted<'_> {
    fn drop(&mut self) {
        if self.armed && self.recursive {
            let _ = self.mount.unmount_recursive(&self.path);
        } else if self.armed {
            let _ = self.mount.unmount(&self.path, self.output);
        }
    }
}

#[cfg(test)]
mod tests;
