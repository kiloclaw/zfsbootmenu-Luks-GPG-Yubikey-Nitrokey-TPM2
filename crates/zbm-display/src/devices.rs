//! How the console on the display opens its devices (ROADMAP.md SE11, ADR
//! 0012): the kinds it opens, how each is opened, which device numbers are
//! of each kind, and the port it opens them through. In the image the
//! console is not root, and asks the supervisor, which checks the device
//! and opens it ([`Opener`] is then the broker of `zbm_app::devices`); on a
//! machine where it may open them itself, and in tests, it opens them
//! itself ([`Direct`]).

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::sync::Arc;

/// `O_NOCTTY`, the same on every Linux architecture.
const O_NOCTTY: i32 = 0o400;

/// A kind of device the console opens.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A DRM card, `/dev/dri/cardN`.
    Card,
    /// A virtual terminal, `/dev/ttyN` (1 to 63).
    Terminal,
    /// The text of a virtual terminal, `/dev/vcsaN` (1 to 63), for
    /// `brltty` (UI8).
    Text,
    /// An input device, `/dev/input/eventN`: a keyboard, the PC speaker.
    Input,
    /// A serial terminal: `ttyS`, `ttyUSB`, `ttyACM`, `ttyAMA` or `hvc`.
    Serial,
}

/// The kinds, in order.
pub const KINDS: [Kind; 5] = [
    Kind::Card,
    Kind::Terminal,
    Kind::Text,
    Kind::Input,
    Kind::Serial,
];

impl Kind {
    /// The device directories and names of this kind below `/dev`, as
    /// (directory, prefix): a name is the prefix and a number.
    fn names(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Kind::Card => &[("dri", "card")],
            Kind::Terminal => &[("", "tty")],
            Kind::Text => &[("", "vcsa")],
            Kind::Input => &[("input", "event")],
            Kind::Serial => &[
                ("", "ttyS"),
                ("", "ttyUSB"),
                ("", "ttyACM"),
                ("", "ttyAMA"),
                ("", "hvc"),
            ],
        }
    }

    /// Whether `name`, a path below `/dev` with `/` between its parts, is a
    /// name of this kind: one of its prefixes and a number,
    /// without leading zeros, and nothing else.
    pub fn is_name(self, name: &str) -> bool {
        let (directory, file) = name.rsplit_once('/').unwrap_or(("", name));
        self.names().iter().any(|(dir, prefix)| {
            *dir == directory
                && file.strip_prefix(prefix).is_some_and(|number| {
                    !number.is_empty()
                        && number.bytes().all(|byte| byte.is_ascii_digit())
                        && (number == "0" || !number.starts_with('0'))
                        // `tty0` and `vcsa0` follow the terminal shown.
                        && (!matches!(self, Kind::Terminal | Kind::Text) || number != "0")
                })
        })
    }

    /// Whether a character device numbered `major`:`minor` is of this kind
    /// (the kernel's `Documentation/admin-guide/devices.txt`).
    pub fn is_device(self, major: u32, minor: u32) -> bool {
        match self {
            // The cards; not the render nodes (128 and on).
            Kind::Card => major == 226 && minor < 64,
            Kind::Terminal => major == 4 && (1..64).contains(&minor),
            // `vcsaN` is 128 + N; 128 is the terminal shown, which moves.
            Kind::Text => major == 7 && (129..192).contains(&minor),
            // `eventN`: 64 to 95, then the dynamic minors from 256; not the
            // joysticks and mice before 64.
            Kind::Input => {
                major == 13 && ((64..96).contains(&minor) || (256..1024).contains(&minor))
            }
            Kind::Serial => match major {
                4 => (64..256).contains(&minor),
                166 | 188 | 204 | 229 => true,
                _ => false,
            },
        }
    }

    /// Opens `path` as this kind is opened: for reading and writing, an
    /// input device for reading alone when it cannot be written, a
    /// terminal never as the controlling terminal, a serial terminal
    /// appending (a test's mirror may be a file).
    ///
    /// # Errors
    ///
    /// When it cannot be opened.
    #[allow(
        clippy::disallowed_methods,
        reason = "the one place the console's devices are opened"
    )]
    pub fn open(self, path: &Path) -> io::Result<File> {
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        match self {
            Kind::Card | Kind::Text => {}
            Kind::Terminal => {
                options.custom_flags(O_NOCTTY);
            }
            Kind::Serial => {
                options.append(true).custom_flags(O_NOCTTY);
            }
            Kind::Input => {
                return options
                    .open(path)
                    .or_else(|_| File::open(path))
                    .map_err(|error| in_path(path, &error));
            }
        }
        options.open(path).map_err(|error| in_path(path, &error))
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Card => "card",
            Kind::Terminal => "terminal",
            Kind::Text => "text",
            Kind::Input => "input",
            Kind::Serial => "serial",
        })
    }
}

/// `error`, saying `path`.
fn in_path(path: &Path, error: &io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// What opens the console's devices.
pub trait Opener: Send + Sync + std::fmt::Debug {
    /// Opens the device of `kind` at `path`.
    ///
    /// # Errors
    ///
    /// When it cannot be opened, or may not be.
    fn open(&self, kind: Kind, path: &Path) -> io::Result<File>;
}

/// Opens each device itself, as [`Kind::open`] does.
#[derive(Clone, Copy, Debug, Default)]
pub struct Direct;

impl Opener for Direct {
    fn open(&self, kind: Kind, path: &Path) -> io::Result<File> {
        kind.open(path)
    }
}

/// The opener of a process that opens its devices itself.
pub fn direct() -> Arc<dyn Opener> {
    Arc::new(Direct)
}

#[cfg(test)]
mod tests;
