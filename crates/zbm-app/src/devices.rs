//! The device broker (ROADMAP.md SE11, ADR 0012): the console on the
//! display runs as its own user, and asks the supervisor for each device it
//! draws on or reads, on a socket pair the supervisor made for it. The
//! supervisor opens a device only when its name is of the kind asked
//! ([`Kind::is_name`]) below `/dev`, and the node is a character device of
//! that kind's numbers ([`Kind::is_device`]), before and after it opens it;
//! it sends the descriptor, or why it refused. The console's end is
//! [`Broker`], an [`Opener`].
//!
//! Each message is JSON, as the other channels' (ADR 0004): a request
//! `{"version": 1, "open": KIND, "path": PATH}`, and a reply
//! `{"version": 1, "reply": "opened"}` with the descriptor, or
//! `{"version": 1, "reply": "refused", "why": WHY}`.

use std::fs::File;
use std::io;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde_json::json;
use zbm_display::devices::{Kind, Opener};
use zbm_sys::socket::Connection;

use crate::wire::{message, text};

/// The version of the messages.
pub const VERSION: u64 = 1;

/// How long the console waits for a device: the supervisor opens it at
/// once, but a card may take a while.
pub const OPEN_WAIT: Duration = Duration::from_secs(10);

/// A request: the device of `kind` at `path`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    /// The kind asked for.
    pub kind: Kind,
    /// The device's path.
    pub path: PathBuf,
}

impl Request {
    /// The message.
    pub fn encode(&self) -> Vec<u8> {
        json!({"version": VERSION, "open": self.kind, "path": self.path})
            .to_string()
            .into_bytes()
    }

    /// The request in `bytes`.
    ///
    /// # Errors
    ///
    /// Why it is not one of this version.
    pub fn decode(bytes: &[u8]) -> Result<Request, String> {
        let (version, object) = message(bytes).ok_or("not a message")?;
        if version != VERSION {
            return Err(format!("version {version}"));
        }
        let kind = object
            .get("open")
            .and_then(|kind| serde_json::from_value(kind.clone()).ok())
            .ok_or("no kind")?;
        let path = text(&object, "path").ok_or("no path")?;
        Ok(Request {
            kind,
            path: PathBuf::from(path),
        })
    }
}

/// A reply without its descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reply {
    /// Opened: the descriptor comes with it.
    Opened,
    /// Refused, and why.
    Refused(String),
}

impl Reply {
    /// The message.
    pub fn encode(&self) -> Vec<u8> {
        let value = match self {
            Reply::Opened => json!({"version": VERSION, "reply": "opened"}),
            Reply::Refused(why) => json!({"version": VERSION, "reply": "refused", "why": why}),
        };
        value.to_string().into_bytes()
    }

    /// The reply in `bytes`; `None` when it is not one.
    pub fn decode(bytes: &[u8]) -> Option<Reply> {
        let (version, object) = message(bytes)?;
        if version != VERSION {
            return None;
        }
        match text(&object, "reply")? {
            "opened" => Some(Reply::Opened),
            "refused" => Some(Reply::Refused(text(&object, "why")?.to_owned())),
            _ => None,
        }
    }
}

/// The major and minor numbers of the device number `rdev`, as glibc's
/// `major` and `minor` take them apart.
pub fn numbers(rdev: u64) -> (u32, u32) {
    let major = ((rdev >> 8) & 0xfff) | ((rdev >> 32) & !0xfff);
    let minor = (rdev & 0xff) | ((rdev >> 12) & !0xff);
    (
        u32::try_from(major).unwrap_or(u32::MAX),
        u32::try_from(minor).unwrap_or(u32::MAX),
    )
}

/// The name of `path` below `dev`, with `/` between its parts, when it is
/// one of `kind`'s: no `..`, `.` or extra `/`.
fn name_below(kind: Kind, path: &Path, dev: &Path) -> Option<String> {
    let rest = path.strip_prefix(dev).ok()?;
    let parts: Option<Vec<&str>> = rest
        .components()
        .map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect();
    let name = parts?.join("/");
    (kind.is_name(&name) && dev.join(&name) == path).then_some(name)
}

/// Why `metadata` is not a device of `kind`; `None` when it is.
fn not_of_kind(kind: Kind, metadata: &std::fs::Metadata) -> Option<String> {
    if !metadata.file_type().is_char_device() {
        return Some("not a character device".to_owned());
    }
    let (major, minor) = numbers(metadata.rdev());
    (!kind.is_device(major, minor)).then(|| format!("device {major}:{minor} is not a {kind}"))
}

/// Opens the device of `kind` at `path` below `dev`, `/dev` in the image,
/// when it is one: its name is of the kind, and it is a character device
/// of the kind's numbers, not a link, both before it is opened and as
/// opened. A device of another kind is never opened: opening some, as a
/// watchdog, does something.
///
/// # Errors
///
/// Why it was refused, or why it could not be opened.
#[allow(
    clippy::disallowed_methods,
    reason = "a device node looked at before it is opened: no file the Files interface reads"
)]
pub fn open_checked(kind: Kind, path: &Path, dev: &Path) -> io::Result<File> {
    let refused = |why: String| io::Error::new(io::ErrorKind::PermissionDenied, why);
    let name = name_below(kind, path, dev).ok_or_else(|| {
        refused(format!(
            "{} is not a {kind} below {}",
            path.display(),
            dev.display()
        ))
    })?;
    let before = std::fs::symlink_metadata(path)?;
    if let Some(why) = not_of_kind(kind, &before) {
        return Err(refused(format!("{name}: {why}")));
    }
    let file = kind.open(path)?;
    // Another node put there meanwhile.
    let after = file.metadata()?;
    let moved = (after.rdev() != before.rdev()).then(|| "changed as it was opened".to_owned());
    if let Some(why) = not_of_kind(kind, &after).or(moved) {
        return Err(refused(format!("{name}: {why}")));
    }
    Ok(file)
}

/// Answers the requests on `connection` with `open` until the other end
/// closes it, or it fails; tells `log` of each refusal.
pub fn serve(
    connection: &Connection,
    open: impl Fn(Kind, &Path) -> io::Result<File>,
    log: impl Fn(&str),
) {
    while let Ok(Some(bytes)) = connection.receive(None) {
        if bytes.is_empty() {
            return;
        }
        let sent = match Request::decode(&bytes) {
            Ok(request) => match open(request.kind, &request.path) {
                Ok(file) => connection
                    .send_with(&Reply::Opened.encode(), &[std::os::fd::AsFd::as_fd(&file)]),
                Err(error) => {
                    log(&format!("refused the console a {}: {error}", request.kind));
                    connection.send(&Reply::Refused(error.to_string()).encode())
                }
            },
            Err(why) => {
                log(&format!("refused the console a request: {why}"));
                connection.send(&Reply::Refused(why).encode())
            }
        };
        if sent.is_err() {
            return;
        }
    }
}

/// The console's end of the broker: an [`Opener`] that asks the supervisor.
#[derive(Debug)]
pub struct Broker {
    connection: Mutex<Connection>,
    /// How long it waits for each device: [`OPEN_WAIT`].
    wait: Duration,
}

impl Broker {
    /// The broker on `connection`.
    pub fn new(connection: Connection) -> Broker {
        Broker {
            connection: Mutex::new(connection),
            wait: OPEN_WAIT,
        }
    }

    /// The broker, waiting `wait` for each device.
    #[must_use]
    pub fn with_wait(mut self, wait: Duration) -> Broker {
        self.wait = wait;
        self
    }
}

impl Opener for Broker {
    fn open(&self, kind: Kind, path: &Path) -> io::Result<File> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let request = Request {
            kind,
            path: path.to_owned(),
        };
        connection.send(&request.encode())?;
        let (bytes, mut fds) = connection.receive_with(Some(self.wait))?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::TimedOut, "the supervisor did not answer")
        })?;
        let reply = Reply::decode(&bytes)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a reply"))?;
        match (reply, fds.len()) {
            (Reply::Opened, 1) => fds
                .pop()
                .map(File::from)
                .ok_or_else(|| io::ErrorKind::InvalidData.into()),
            (Reply::Opened, _) => Err(io::Error::new(io::ErrorKind::InvalidData, "no descriptor")),
            // Not `PermissionDenied`, which a seat takes for another
            // program holding the display, and waits on.
            (Reply::Refused(why), _) => Err(io::Error::other(format!("{}: {why}", path.display()))),
        }
    }
}

#[cfg(test)]
mod tests;
