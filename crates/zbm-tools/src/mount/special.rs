//! The kernel's file systems a chroot takes, each a [`Mounted`]; a file
//! of this system bound read-only and a `tmpfs`, for the chroot's name
//! servers; and [`HostEfivars`], this system's `efivarfs` in the chroot's
//! mode for as long as it lives.

use std::path::Path;

use zbm_platform::Command;

use super::{Mount, Mounted, Shown, shell_code, shown};

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

    /// Mounts `source` again on `target`, read-only: `mount -B SOURCE
    /// TARGET`, then `mount -o remount,bind,ro TARGET`, which makes this
    /// mount read-only and leaves the file system it came from as it is.
    /// A bind whose remount fails is unmounted at once. Unmounting it is
    /// quiet, as for [`Mount::bind`].
    ///
    /// # Errors
    ///
    /// [`NotReadOnly`]: the exit status of the `mount` that failed, 126 when
    /// it could not start, and the bind itself when it could be neither
    /// remounted nor unmounted, which is then mounted read/write.
    pub fn bind_read_only(
        self,
        source: &Path,
        target: &Path,
    ) -> Result<Mounted<'a>, NotReadOnly<'a>> {
        let bound = self.bind(source, target).map_err(|code| NotReadOnly {
            code,
            writable: None,
        })?;
        let remount = Command::new("mount")
            .args(["-o", "remount,bind,ro"])
            .arg(target);
        let code = shell_code(&self.runner.run(&shown(remount, Shown::Console)));
        if code == 0 {
            return Ok(bound);
        }
        let writable = if self.unmount(target, Shown::Hidden) {
            let _ = bound.leave();
            None
        } else {
            Some(bound)
        };
        Err(NotReadOnly { code, writable })
    }

    /// Mounts a `tmpfs` on `target`, writable by root alone, with no
    /// set-user-ID programs and no devices (`mount -t tmpfs -o
    /// mode=0755,nosuid,nodev tmpfs TARGET`).
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start.
    pub fn tmpfs(self, target: &Path) -> Result<Mounted<'a>, i32> {
        let tmpfs = ["-t", "tmpfs", "-o", "mode=0755,nosuid,nodev", "tmpfs"];
        self.special(Command::new("mount").args(tmpfs), target)
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

/// Why [`Mount::bind_read_only`] mounted nothing read-only.
#[must_use = "a bind left writable is unmounted when this is dropped"]
pub struct NotReadOnly<'a> {
    /// The exit status of the `mount` that failed, 126 when it could not
    /// start.
    pub code: i32,
    /// The bind, mounted read/write, when its remount failed and it could
    /// not be unmounted: dropping it tries again.
    pub writable: Option<Mounted<'a>>,
}

/// This system's `efivarfs`, mounted or remounted in a chroot's mode,
/// which dropping it remounts read-only, as start-up left it.
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
        Self::with_options(mount, &format!("remount,{mode}"))
    }

    /// Mounts this system's `efivarfs`, which is not mounted, in `mode`,
    /// `ro` or `rw` (`mount -t efivarfs efivarfs /sys/firmware/efi/efivars
    /// -o MODE`), its output on the console; dropping it leaves it mounted,
    /// read-only.
    ///
    /// # Errors
    ///
    /// The exit status of `mount` when it failed, 126 when it could not
    /// start; nothing is mounted then.
    pub fn mount(mount: Mount<'a>, mode: &str) -> Result<HostEfivars<'a>, i32> {
        Self::with_options(mount, mode)
    }

    /// Mounts this system's `efivarfs` with `options`.
    fn with_options(mount: Mount<'a>, options: &str) -> Result<HostEfivars<'a>, i32> {
        match mount.efivarfs_status(Path::new(EFIVARS), options) {
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
