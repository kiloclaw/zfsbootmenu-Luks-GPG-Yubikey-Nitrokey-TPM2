//! The virtual terminal under the display: while the menu draws, the kernel
//! console must neither draw over it nor take its keys; afterwards it gets
//! both back, so that the recovery shell or the booted kernel find the
//! console as they expect.

#![allow(
    clippy::disallowed_methods,
    reason = "the virtual terminal is this module's system interface: it opens it and reads sysfs"
)]

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use zbm_sys::console::{
    DisplayMode, KeyboardMode, keyboard_mode, set_display_mode, set_keyboard_mode,
};

/// `O_NOCTTY`, the same on every Linux architecture.
const O_NOCTTY: i32 = 0o400;

/// The virtual terminal shown now, from sysfs (`/sys/class/tty/tty0/active`,
/// such as `tty1`), as a path below `dev`.
pub fn active_terminal(sysfs: &Path, dev: &Path) -> Option<PathBuf> {
    let active = std::fs::read_to_string(sysfs.join("class/tty/tty0/active")).ok()?;
    let name = active.trim();
    (name.starts_with("tty") && !name.contains('/')).then(|| dev.join(name))
}

/// A virtual terminal taken over: the kernel console draws nothing on it and
/// its keys reach no program, until this is dropped.
#[derive(Debug)]
pub struct Terminal {
    file: File,
    /// The keyboard mode it had, given back when dropped.
    keyboard: KeyboardMode,
}

impl Terminal {
    /// Takes over the virtual terminal at `path`: graphics mode, and its
    /// keyboard off.
    ///
    /// # Errors
    ///
    /// When it cannot be opened or is not a virtual terminal.
    pub fn take(path: &Path) -> io::Result<Terminal> {
        // O_NOCTTY: never this process's controlling terminal.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(O_NOCTTY)
            .open(path)?;
        let fd = file.as_raw_fd();
        let keyboard = keyboard_mode(fd)?;
        set_keyboard_mode(fd, KeyboardMode::OFF)?;
        if let Err(error) = set_display_mode(fd, DisplayMode::Graphics) {
            let _ = set_keyboard_mode(fd, keyboard);
            return Err(error);
        }
        Ok(Terminal { file, keyboard })
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let fd = self.file.as_raw_fd();
        let _ = set_display_mode(fd, DisplayMode::Text);
        let _ = set_keyboard_mode(fd, self.keyboard);
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_active_terminal_is_read_from_sysfs() {
        let root = std::env::temp_dir().join(format!("zbm-display-vt-{}", std::process::id()));
        let tty0 = root.join("class/tty/tty0");
        std::fs::create_dir_all(&tty0).unwrap();

        std::fs::write(tty0.join("active"), "tty2\n").unwrap();
        assert_eq!(
            active_terminal(&root, Path::new("/dev")),
            Some(PathBuf::from("/dev/tty2"))
        );
        std::fs::write(tty0.join("active"), "../x\n").unwrap();
        assert_eq!(active_terminal(&root, Path::new("/dev")), None);
        assert_eq!(active_terminal(&root.join("none"), Path::new("/dev")), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn what_is_not_a_virtual_terminal_cannot_be_taken() {
        assert!(Terminal::take(Path::new("/dev/null")).is_err());
        assert!(Terminal::take(Path::new("/nonexistent/tty1")).is_err());
    }
}
