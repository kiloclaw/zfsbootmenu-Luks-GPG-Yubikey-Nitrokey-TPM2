//! Where the console finds the system, and how it opens its devices there.

use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use zbm_display::devices::Kind;

use super::Devices;

/// Where the console finds the system.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Places {
    /// sysfs: the virtual terminal shown, the keyboards.
    pub sys: PathBuf,
    /// The device files: the DRM cards, the input devices.
    pub dev: PathBuf,
    /// The runtime's state directory, which holds the console's process ID.
    pub base: PathBuf,
    /// The kernel log.
    pub kmsg: PathBuf,
    /// The control socket.
    pub socket: PathBuf,
    /// How it opens its devices, and the kernel log it was given.
    pub devices: Devices,
}

impl Places {
    /// The system's.
    pub fn system() -> Places {
        Places {
            sys: PathBuf::from("/sys"),
            dev: PathBuf::from("/dev"),
            base: PathBuf::from(zbm_state::BASE),
            kmsg: PathBuf::from("/dev/kmsg"),
            socket: PathBuf::from(zbm_app::console::SOCKET),
            devices: Devices::direct(),
        }
    }

    /// Writes `text` to the kernel log at `level`, to the one it was given
    /// when it has one; failures are ignored.
    pub fn log(&self, level: u8, text: &str) {
        let line = format!("<{level}>ZFSBootMenu: console: {text}\n");
        match &self.devices.kmsg {
            Some(kmsg) => {
                let _ = (&**kmsg).write_all(line.as_bytes());
            }
            None => {
                if let Ok(mut kmsg) = OpenOptions::new().append(true).open(&self.kmsg) {
                    let _ = kmsg.write_all(line.as_bytes());
                }
            }
        }
    }

    /// Opens the device of `kind` at `path`, as [`Places::devices`] says.
    ///
    /// # Errors
    ///
    /// When it cannot be opened, or may not be.
    pub fn open(&self, kind: Kind, path: &Path) -> io::Result<File> {
        self.devices.opener.open(kind, path)
    }
}
