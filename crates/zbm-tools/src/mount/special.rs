//! The kernel's file systems a chroot takes, each a [`Mounted`], and
//! [`HostEfivars`], this system's `efivarfs` in the chroot's mode for as
//! long as it lives.

use std::path::Path;

use zbm_platform::Command;

use super::{Mount, Mounted, Shown, shown};

/// Where this system's `efivarfs` is mounted.
const EFIVARS: &str = "/sys/firmware/efi/efivars";

impl<'a> Mount<'a> {
    /// Mounts what `command` mounts on `target`, its output on the console;
    /// unmounting it is quiet, and its failure ignored, for a target the
    /// user or an outer unmount took away already.
    fn special(self, command: Command, target: &Path) -> Result<Mounted<'a>, i32> {
        let run = self.runner.run(&shown(command.arg(target), Shown::Console));
        self.mounted(&run, target).map(Mounted::quietly)
    }

    /// Mounts `source` again on `target` (`mount -B SOURCE TARGET`).
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn bind(self, source: &Path, target: &Path) -> Result<Mounted<'a>, i32> {
        self.special(Command::new("mount").arg("-B").arg(source), target)
    }

    /// Mounts a `proc` on `target` (`mount -t proc proc TARGET`).
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn proc(self, target: &Path) -> Result<Mounted<'a>, i32> {
        self.special(Command::new("mount").args(["-t", "proc", "proc"]), target)
    }

    /// Mounts a `sysfs` on `target` (`mount -t sysfs sys TARGET`).
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn sysfs(self, target: &Path) -> Result<Mounted<'a>, i32> {
        self.special(Command::new("mount").args(["-t", "sysfs", "sys"]), target)
    }

    /// Mounts a `devpts` on `target` (`mount -t devpts pts TARGET`).
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn devpts(self, target: &Path) -> Result<Mounted<'a>, i32> {
        self.special(Command::new("mount").args(["-t", "devpts", "pts"]), target)
    }

    /// Mounts an `efivarfs` on `target` in `mode`, `ro` or `rw`
    /// (`mount -t efivarfs efivarfs TARGET -o MODE`), as [`Mount::efivarfs`]
    /// does, its output on the console; unmounting it is quiet.
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn efivarfs_in(self, target: &Path, mode: &str) -> Result<Mounted<'a>, i32> {
        match self.efivarfs_status(target, mode) {
            0 => Ok(Mounted::new(self, target).quietly()),
            code => Err(code),
        }
    }
}

/// This system's `efivarfs`, remounted in a chroot's mode, which dropping
/// it remounts read-only, as start-up left it.
#[must_use = "dropping it sets the efivarfs back to read-only at once"]
pub struct HostEfivars<'a> {
    mount: Mount<'a>,
}

impl<'a> HostEfivars<'a> {
    /// Remounts this system's `efivarfs` in `mode`, `ro` or `rw`
    /// (`mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o
    /// remount,MODE`), its output on the console.
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start; the `efivarfs` is then as it was.
    pub fn remount(mount: Mount<'a>, mode: &str) -> Result<HostEfivars<'a>, i32> {
        match mount.efivarfs_status(Path::new(EFIVARS), &format!("remount,{mode}")) {
            0 => Ok(HostEfivars { mount }),
            code => Err(code),
        }
    }
}

impl Drop for HostEfivars<'_> {
    fn drop(&mut self) {
        let _ = self.mount.efivarfs(EFIVARS, "remount,ro");
    }
}

#[cfg(test)]
mod tests;
