//! The session's socket (ROADMAP.md, PM10; ADR 0002): another menu, as
//! over SSH, takes the session's menu over by a handshake, `Takeover` →
//! `Yielded`, instead of a signal to whatever the menu runs. The session
//! ends its menu, which lets go of the `active` lock, and then says it
//! yielded; the other menu takes the lock.
//!
//! The socket is `SOCK_SEQPACKET`, one message a packet, at [`SOCKET`],
//! for root only: the kernel names each peer, and one that is not the
//! session's own user is refused. Each message is a JSON object with the
//! [`VERSION`] of the protocol, so that a session and a menu of different
//! images know when they do not understand each other:
//!
//! ```text
//! {"version":1,"request":"takeover"}
//! {"version":1,"reply":"yielded"}
//! {"version":1,"reply":"refused","reason":"busy"}
//! ```
//!
//! A menu that is to be a client of the session asks for the menu as
//! `remote` instead: the session's menu yields as for a takeover, and the
//! connection goes on in the protocol of `zbm_app::remote`, served on a
//! thread of its own by what [`SessionSocket::serve_with`] was given.

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};
use zbm_platform::event::{CancelToken, spawn_scoped_traced};
use zbm_sys::socket::{Connection, Listener};

use crate::wire::{self, Unread, message};

/// Where the session listens, beside the console's socket (ADR 0004).
pub const SOCKET: &str = "/run/zfsbootmenu/session.sock";

/// The version of the protocol.
pub const VERSION: u64 = 1;

/// How long the session waits for its menu to end once asked.
pub const YIELD_WAIT: Duration = Duration::from_secs(4);

/// How long a menu waits for the session's answer: a moment longer than
/// the session waits for its menu.
pub const ANSWER_WAIT: Duration = Duration::from_secs(5);

/// What a menu asks the session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    /// End the menu, if one runs, and say so.
    Takeover,
    /// End the menu, as [`Request::Takeover`], and serve one for the peer
    /// on this connection (`zbm_app::remote`).
    Remote,
}

/// Why the session refused a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The peer is not the session's user, root.
    NotRoot,
    /// The request is of another version; the reply's is the session's.
    Version,
    /// The request is not one of the protocol.
    Malformed,
    /// The menu did not end in time ([`YIELD_WAIT`]), as when a program it
    /// runs holds the terminal.
    Busy,
    /// The session serves no remote menu: the peer runs its own.
    NoRemote,
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// What the session answers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Answer {
    /// No menu of the session runs any more, nor holds the `active` lock.
    Yielded,
    /// The request was refused.
    Refused(Refusal),
}

impl Refusal {
    /// Its name in a message.
    fn name(self) -> &'static str {
        match self {
            Refusal::NotRoot => "not root",
            Refusal::Version => "version",
            Refusal::Malformed => "malformed",
            Refusal::Busy => "busy",
            Refusal::NoRemote => "no remote",
        }
    }

    /// The refusal named `name`.
    fn named(name: &str) -> Option<Refusal> {
        [
            Refusal::NotRoot,
            Refusal::Version,
            Refusal::Malformed,
            Refusal::Busy,
            Refusal::NoRemote,
        ]
        .into_iter()
        .find(|refusal| refusal.name() == name)
    }
}

impl Request {
    /// The message that carries it.
    pub fn encode(self) -> Vec<u8> {
        let name = match self {
            Request::Takeover => "takeover",
            Request::Remote => "remote",
        };
        json!({"version": VERSION, "request": name})
            .to_string()
            .into_bytes()
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
        match object.get("request").and_then(Value::as_str) {
            Some("takeover") => Ok(Request::Takeover),
            Some("remote") => Ok(Request::Remote),
            _ => Err(Refusal::Malformed),
        }
    }
}

impl Answer {
    /// The message that carries it.
    pub fn encode(self) -> Vec<u8> {
        let value = match self {
            Answer::Yielded => json!({"version": VERSION, "reply": "yielded"}),
            Answer::Refused(refusal) => {
                json!({"version": VERSION, "reply": "refused", "reason": refusal.name()})
            }
        };
        value.to_string().into_bytes()
    }

    /// The answer the session sent as `bytes`, and its version; `None` when
    /// it is none of the protocol.
    pub fn decode(bytes: &[u8]) -> Option<(u64, Answer)> {
        let (version, object) = message(bytes)?;
        let answer = match object.get("reply")?.as_str()? {
            "yielded" => Answer::Yielded,
            "refused" => Answer::Refused(Refusal::named(wire::text(&object, "reason")?)?),
            _ => return None,
        };
        Some((version, answer))
    }
}

/// Whether the session's menu runs, and whether a takeover was asked of it.
#[derive(Debug, Default)]
struct Menu {
    running: bool,
    asked: bool,
}

/// What the session's menu and the socket share: the menu says when it
/// runs, and looks whether it was asked to end; the socket asks it to, and
/// waits for it to end.
#[derive(Debug, Default)]
pub struct Handover {
    menu: Mutex<Menu>,
    ended: Condvar,
}

impl Handover {
    /// The state, whatever a thread that panicked left in it.
    fn menu(&self) -> MutexGuard<'_, Menu> {
        self.menu.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A menu starts: no takeover was asked of it yet.
    pub fn enter(&self) {
        *self.menu() = Menu {
            running: true,
            asked: false,
        };
    }

    /// The menu ended, and let go of the `active` lock.
    pub fn leave(&self) {
        *self.menu() = Menu::default();
        self.ended.notify_all();
    }

    /// Whether the menu that runs was asked to end.
    pub fn is_asked(&self) -> bool {
        let menu = self.menu();
        menu.running && menu.asked
    }

    /// Asks the menu that runs, if one does, to end, and waits at most
    /// `wait` for it to; whether none runs. The session's own menu asks
    /// a remote menu it serves so, to take the menu back.
    pub fn has_yielded(&self, wait: Duration) -> bool {
        let mut menu = self.menu();
        if menu.running {
            menu.asked = true;
        }
        let (menu, _) = self
            .ended
            .wait_timeout_while(menu, wait, |menu| menu.running)
            .unwrap_or_else(PoisonError::into_inner);
        !menu.running
    }
}

/// What serves a remote menu on the connection it asked for one on.
pub type RemoteMenu<'r> = dyn Fn(Connection) + Sync + 'r;

/// The session's socket: it answers the peers that connect, one at a time,
/// from the thread that runs [`SessionSocket::serve`].
#[derive(Debug)]
pub struct SessionSocket {
    listener: Listener,
    handover: Handover,
    /// The user the peers must be, the session's: root.
    user: u32,
    /// How long it waits for the menu to end: [`YIELD_WAIT`].
    yield_wait: Duration,
}

impl SessionSocket {
    /// The socket at `path`.
    ///
    /// # Errors
    ///
    /// When it cannot be made, or another session listens there.
    pub fn bind(path: &Path) -> io::Result<SessionSocket> {
        Ok(SessionSocket {
            listener: Listener::bind(path)?,
            handover: Handover::default(),
            user: zbm_sys::credentials().uid,
            yield_wait: YIELD_WAIT,
        })
    }

    /// The socket, which waits `wait` for the menu to end instead of
    /// [`YIELD_WAIT`].
    #[must_use]
    pub fn with_yield_wait(mut self, wait: Duration) -> SessionSocket {
        self.yield_wait = wait;
        self
    }

    /// What it shares with the session's menu.
    pub fn handover(&self) -> &Handover {
        &self.handover
    }

    /// Answers the peers that connect until `until` is cancelled; it
    /// serves no remote menu.
    pub fn serve(&self, until: &CancelToken) {
        self.serve_with(until, None);
    }

    /// Answers the peers that connect until `until` is cancelled, and has
    /// `remote` serve the menu of each peer that asks for one, on a thread
    /// of its own, which ends before this returns: `remote` ends when
    /// `until` is cancelled, or when the menu it serves is asked to end
    /// ([`Handover::is_asked`]).
    pub fn serve_with(&self, until: &CancelToken, remote: Option<&RemoteMenu<'_>>) {
        std::thread::scope(|scope| {
            wire::accept_until(&self.listener, until, "session", |peer| {
                if let (Some(peer), Some(remote)) = (self.answer(peer, remote.is_some()), remote) {
                    spawn_scoped_traced(scope, move || remote(peer));
                }
            });
        });
    }

    /// Answers the request of `peer`; the connection, when it is a remote
    /// menu's that the session is to serve, which it can when `remote`.
    fn answer(&self, peer: Connection, remote: bool) -> Option<Connection> {
        let request = self.request_of(&peer);
        let answer = match request {
            Ok(Request::Remote) if !remote => Answer::Refused(Refusal::NoRemote),
            Ok(Request::Takeover | Request::Remote) => {
                tracing::info!("another menu takes the menu over");
                if self.handover.has_yielded(self.yield_wait) {
                    Answer::Yielded
                } else {
                    Answer::Refused(Refusal::Busy)
                }
            }
            Err(refusal) => Answer::Refused(refusal),
        };
        if let Err(error) = peer.send(&answer.encode()) {
            tracing::debug!(%error, "the peer went before the answer");
            return None;
        }
        (request == Ok(Request::Remote) && answer == Answer::Yielded).then_some(peer)
    }

    /// What `peer` asks, unless it is refused.
    fn request_of(&self, peer: &Connection) -> Result<Request, Refusal> {
        match wire::request_of(peer, self.user) {
            Ok(bytes) => Request::decode(&bytes),
            Err(Unread::Stranger) => Err(Refusal::NotRoot),
            Err(Unread::Nothing) => Err(Refusal::Malformed),
        }
    }
}

/// Asks the session listening at `path` to end its menu; its answer, `None`
/// when no session listens there or it did not answer in [`ANSWER_WAIT`].
pub fn ask_takeover(path: &Path) -> Option<Answer> {
    let session = Connection::connect(path).ok()?;
    session.send(&Request::Takeover.encode()).ok()?;
    let bytes = session.receive(Some(ANSWER_WAIT)).ok()??;
    let (version, answer) = Answer::decode(&bytes)?;
    (version == VERSION || answer == Answer::Refused(Refusal::Version)).then_some(answer)
}

/// Asks the session listening at `path` to serve this process a menu: the
/// connection, which goes on in the protocol of `zbm_app::remote`; else
/// the session's answer, `None` when no session listens there or it did not
/// answer in [`ANSWER_WAIT`].
///
/// # Errors
///
/// The session's answer when it serves no menu.
pub fn ask_remote(path: &Path) -> Result<Connection, Option<Answer>> {
    let session = Connection::connect(path).map_err(|_| None)?;
    session.send(&Request::Remote.encode()).map_err(|_| None)?;
    let bytes = session
        .receive(Some(ANSWER_WAIT))
        .ok()
        .flatten()
        .ok_or(None)?;
    match Answer::decode(&bytes) {
        Some((VERSION, Answer::Yielded)) => Ok(session),
        Some((_, answer)) => Err(Some(answer)),
        None => Err(None),
    }
}

#[cfg(test)]
mod tests;
