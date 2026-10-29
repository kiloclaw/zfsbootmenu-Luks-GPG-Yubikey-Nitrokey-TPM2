//! The console's control socket (ROADMAP.md, PM7; ADR 0004): what other
//! processes ask `zbm console`, the console on the display, each request
//! with a reply that says it was done. No signal, no record to poll.
//!
//! The socket is `SOCK_SEQPACKET`, one message a packet, at [`SOCKET`],
//! for root only, as the session's ([`crate::session`]). Each message is a
//! JSON object with the [`VERSION`] of the protocol:
//!
//! ```text
//! {"version":1,"request":"release"}
//!     {"version":1,"reply":"released"}
//! {"version":1,"request":"reattach"}
//!     {"version":1,"reply":"reattached"}
//! {"version":1,"request":"layout","name":"de"}
//!     {"version":1,"reply":"layout set","name":"de"}
//!     {"version":1,"reply":"no such layout","name":"de"}
//! {"version":1,"request":"font","size":"+"}
//!     {"version":1,"reply":"font set","font":"…"}
//! {"version":1,"request":"status"}
//!     {"version":1,"reply":"status","shown":true,
//!      "layout":"us","font":"…"}
//! {"version":1,"request":"secret"}
//!     {"version":1,"reply":"secret held"}
//! any request, refused:
//!     {"version":1,"reply":"refused","reason":"busy"}
//! ```
//!
//! `secret` is held for as long as the connection stays open: the menu
//! asks it while it shows a secret prompt that the console cannot see, and
//! the console keeps no scrollback and shows nothing being typed until the
//! menu closes the connection, or ends.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value, json};
use zbm_sys::socket::Connection;

use crate::wire::{message, text};

/// Where the console listens, beside the session (ADR 0004).
pub const SOCKET: &str = "/run/zfsbootmenu/console.sock";

/// The version of the protocol.
pub const VERSION: u64 = 1;

/// How long the console waits for its loop to do what is asked, before it
/// answers that it is busy.
pub const DONE_WAIT: Duration = Duration::from_secs(10);

/// How long a secret's holder waits for the console to note it.
const HOLD_WAIT: Duration = Duration::from_secs(1);

/// What a process asks the console.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    /// Hand the display, the keyboards and the virtual terminal back, as
    /// before `kexec`.
    Release,
    /// Take the display again, after a `kexec` that failed.
    Reattach,
    /// Type with the image's keyboard layout of this name.
    Layout(String),
    /// Draw with the font's size this asks for: a height in pixels, `+`,
    /// `-` or `default`.
    Font(String),
    /// Say how the console is.
    Status,
    /// A secret prompt is shown, which the console cannot see, for as long
    /// as the connection stays open.
    Secret,
}

/// How the console is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Status {
    /// Whether it draws on the display, rather than relaying on the kernel
    /// console.
    pub shown: bool,
    /// The keyboard layout it types with.
    pub layout: String,
    /// The font it draws with, and the columns and rows.
    pub font: String,
}

/// Why the console refused a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The peer is not the console's user, root.
    NotRoot,
    /// The request is of another version; the reply's is the console's.
    Version,
    /// The request is not one of the protocol.
    Malformed,
    /// The console does not draw on the display, so it reads no keyboard
    /// and has no font.
    NotShown,
    /// The console did not do it in time ([`DONE_WAIT`]), or is ending.
    Busy,
}

/// The refusals, for their names.
const REFUSALS: [Refusal; 5] = [
    Refusal::NotRoot,
    Refusal::Version,
    Refusal::Malformed,
    Refusal::NotShown,
    Refusal::Busy,
];

impl Refusal {
    /// Its name in a message.
    fn name(self) -> &'static str {
        match self {
            Refusal::NotRoot => "not root",
            Refusal::Version => "version",
            Refusal::Malformed => "malformed",
            Refusal::NotShown => "not shown",
            Refusal::Busy => "busy",
        }
    }

    /// The refusal named `name`.
    fn named(name: &str) -> Option<Refusal> {
        REFUSALS.into_iter().find(|refusal| refusal.name() == name)
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// What the console answers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reply {
    /// The display is handed back, or the console held none.
    Released,
    /// The display is taken again, or was never handed back.
    Reattached,
    /// The keyboards type with this layout now.
    LayoutSet(String),
    /// The image has no layout of this name: the keyboards keep theirs.
    NoSuchLayout(String),
    /// The console draws with this font now: the one asked for, or the one
    /// it kept when it had no other size.
    FontSet(String),
    /// How the console is.
    Status(Status),
    /// The secret prompt is noted until the connection closes.
    SecretHeld,
    /// The request was refused.
    Refused(Refusal),
}

/// Whether `name` may name a keyboard layout: a word of the image's list,
/// never a path.
fn is_layout_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && !name.contains(['/', '\0'])
}

impl Request {
    /// The message that carries it.
    pub fn encode(&self) -> Vec<u8> {
        let value = match self {
            Request::Release => json!({"version": VERSION, "request": "release"}),
            Request::Reattach => json!({"version": VERSION, "request": "reattach"}),
            Request::Layout(name) => {
                json!({"version": VERSION, "request": "layout", "name": name})
            }
            Request::Font(size) => json!({"version": VERSION, "request": "font", "size": size}),
            Request::Status => json!({"version": VERSION, "request": "status"}),
            Request::Secret => json!({"version": VERSION, "request": "secret"}),
        };
        value.to_string().into_bytes()
    }

    /// The request a peer sent as `bytes`.
    ///
    /// # Errors
    ///
    /// The refusal to answer: a message of another version, or none of the
    /// protocol.
    pub fn decode(bytes: &[u8]) -> Result<Request, Refusal> {
        let (version, object) = message(bytes).ok_or(Refusal::Malformed)?;
        if version != VERSION {
            return Err(Refusal::Version);
        }
        let field = |key| text(&object, key).ok_or(Refusal::Malformed);
        match field("request")? {
            "release" => Ok(Request::Release),
            "reattach" => Ok(Request::Reattach),
            "layout" => Some(field("name")?)
                .filter(|name| is_layout_name(name))
                .map(|name| Request::Layout(name.to_owned()))
                .ok_or(Refusal::Malformed),
            "font" => Ok(Request::Font(field("size")?.to_owned())),
            "status" => Ok(Request::Status),
            "secret" => Ok(Request::Secret),
            _ => Err(Refusal::Malformed),
        }
    }
}

impl Reply {
    /// The message that carries it.
    pub fn encode(&self) -> Vec<u8> {
        let mut object = Map::new();
        object.insert("version".to_owned(), VERSION.into());
        let mut put = |key: &str, value: Value| {
            object.insert(key.to_owned(), value);
        };
        match self {
            Reply::Released => put("reply", "released".into()),
            Reply::Reattached => put("reply", "reattached".into()),
            Reply::LayoutSet(name) => {
                put("reply", "layout set".into());
                put("name", name.as_str().into());
            }
            Reply::NoSuchLayout(name) => {
                put("reply", "no such layout".into());
                put("name", name.as_str().into());
            }
            Reply::FontSet(font) => {
                put("reply", "font set".into());
                put("font", font.as_str().into());
            }
            Reply::Status(status) => {
                put("reply", "status".into());
                put("shown", status.shown.into());
                put("layout", status.layout.as_str().into());
                put("font", status.font.as_str().into());
            }
            Reply::SecretHeld => put("reply", "secret held".into()),
            Reply::Refused(refusal) => {
                put("reply", "refused".into());
                put("reason", refusal.name().into());
            }
        }
        Value::Object(object).to_string().into_bytes()
    }

    /// The reply the console sent as `bytes`, and its version; `None` when
    /// it is none of the protocol.
    pub fn decode(bytes: &[u8]) -> Option<(u64, Reply)> {
        let (version, object) = message(bytes)?;
        let field = |key| text(&object, key).map(str::to_owned);
        let reply = match text(&object, "reply")? {
            "released" => Reply::Released,
            "reattached" => Reply::Reattached,
            "layout set" => Reply::LayoutSet(field("name")?),
            "no such layout" => Reply::NoSuchLayout(field("name")?),
            "font set" => Reply::FontSet(field("font")?),
            "status" => Reply::Status(Status {
                shown: object.get("shown")?.as_bool()?,
                layout: field("layout")?,
                font: field("font")?,
            }),
            "secret held" => Reply::SecretHeld,
            "refused" => Reply::Refused(Refusal::named(text(&object, "reason")?)?),
            _ => return None,
        };
        Some((version, reply))
    }
}

/// Why a request got no reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unanswered {
    /// No console listens: none runs, or it ended.
    NoConsole,
    /// The console did not answer in time.
    Late,
    /// It answered something of no version of the protocol it knows.
    Garbled,
}

impl fmt::Display for Unanswered {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Unanswered::NoConsole => "no console on the display",
            Unanswered::Late => "the console did not answer",
            Unanswered::Garbled => "the console's answer was not understood",
        })
    }
}

/// Sends `request` to the console at `path` and waits at most `wait` for
/// its reply; the connection, still open, and the reply.
fn exchange(
    path: &Path,
    request: &Request,
    wait: Duration,
) -> Result<(Connection, Reply), Unanswered> {
    let console = Connection::connect(path).map_err(|_| Unanswered::NoConsole)?;
    console
        .send(&request.encode())
        .map_err(|_| Unanswered::NoConsole)?;
    match console.receive(Some(wait)) {
        Ok(Some(bytes)) if !bytes.is_empty() => {
            let (version, reply) = Reply::decode(&bytes).ok_or(Unanswered::Garbled)?;
            if version == VERSION || reply == Reply::Refused(Refusal::Version) {
                Ok((console, reply))
            } else {
                Err(Unanswered::Garbled)
            }
        }
        Ok(None) => Err(Unanswered::Late),
        // Closed without a reply: the console ended meanwhile.
        Ok(Some(_)) | Err(_) => Err(Unanswered::NoConsole),
    }
}

/// Asks the console listening at `path` for `request`, and waits at most
/// `wait` for its reply.
///
/// # Errors
///
/// When no console listens, or it did not answer in time, or not in its
/// protocol.
pub fn ask(path: &Path, request: &Request, wait: Duration) -> Result<Reply, Unanswered> {
    exchange(path, request, wait).map(|(_, reply)| reply)
}

/// A secret prompt the console knows of: it keeps no scrollback and shows
/// nothing being typed until this is dropped, or the process ends.
#[derive(Debug)]
pub struct SecretHold {
    _console: Connection,
}

/// Tells the console listening at `path` that a secret prompt is shown,
/// until what this returns is dropped; `None` when no console noted it, as
/// when none runs.
pub fn hold_secret(path: &Path) -> Option<SecretHold> {
    match exchange(path, &Request::Secret, HOLD_WAIT) {
        Ok((console, Reply::SecretHeld)) => Some(SecretHold { _console: console }),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
