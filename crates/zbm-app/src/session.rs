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

use std::fmt;
use std::io;
use std::os::fd::AsFd as _;
use std::path::Path;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};
use zbm_platform::event::CancelToken;
use zbm_sys::console::{Watch, wait_ready};
use zbm_sys::socket::{Connection, Listener};

/// Where the session listens, beside the console's socket (ADR 0004).
pub const SOCKET: &str = "/run/zfsbootmenu/session.sock";

/// The version of the protocol.
pub const VERSION: u64 = 1;

/// How long the session waits for its menu to end once asked.
pub const YIELD_WAIT: Duration = Duration::from_secs(4);

/// How long a menu waits for the session's answer: a moment longer than
/// the session waits for its menu.
pub const ANSWER_WAIT: Duration = Duration::from_secs(5);

/// How long the session waits for a peer's request once it connected.
const REQUEST_WAIT: Duration = Duration::from_secs(1);

/// What a menu asks the session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    /// End the menu, if one runs, and say so.
    Takeover,
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
        }
    }

    /// The refusal named `name`.
    fn named(name: &str) -> Option<Refusal> {
        [
            Refusal::NotRoot,
            Refusal::Version,
            Refusal::Malformed,
            Refusal::Busy,
        ]
        .into_iter()
        .find(|refusal| refusal.name() == name)
    }
}

/// The object of a message, with the version it carries.
fn message(bytes: &[u8]) -> Option<(u64, serde_json::Map<String, Value>)> {
    let Ok(Value::Object(object)) = serde_json::from_slice(bytes) else {
        return None;
    };
    let version = object.get("version")?.as_u64()?;
    Some((version, object))
}

impl Request {
    /// The message that carries it.
    pub fn encode(self) -> Vec<u8> {
        let Request::Takeover = self;
        json!({"version": VERSION, "request": "takeover"})
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
            "refused" => Answer::Refused(Refusal::named(object.get("reason")?.as_str()?)?),
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
    /// `wait` for it to; whether none runs.
    fn has_yielded(&self, wait: Duration) -> bool {
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

    /// Answers the peers that connect until `until` is cancelled.
    pub fn serve(&self, until: &CancelToken) {
        loop {
            let watches = [
                Watch::read(self.listener.as_fd()),
                Watch::read(until.as_fd()),
            ];
            let ready = wait_ready(&watches, None);
            if until.is_cancelled() {
                return;
            }
            // A peer that went before it was accepted is none.
            match ready.and_then(|_| self.listener.accept(Some(Duration::ZERO))) {
                Ok(peer) => peer.iter().for_each(|peer| self.answer(peer)),
                Err(error) => {
                    tracing::error!(%error, "the session's socket failed");
                    return;
                }
            }
        }
    }

    /// Answers the request of `peer`.
    fn answer(&self, peer: &Connection) {
        let answer = match self.request_of(peer) {
            Ok(Request::Takeover) => {
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
        }
    }

    /// What `peer` asks, unless it is refused.
    fn request_of(&self, peer: &Connection) -> Result<Request, Refusal> {
        // Read before the answer even when it is refused: a peer whose
        // request was left unread would get no answer, but a reset.
        let request = peer.receive(Some(REQUEST_WAIT));
        let trusted = peer
            .peer()
            .is_ok_and(|credentials| credentials.uid == self.user);
        if !trusted {
            return Err(Refusal::NotRoot);
        }
        match request {
            Ok(Some(bytes)) => Request::decode(&bytes),
            _ => Err(Refusal::Malformed),
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

#[cfg(test)]
mod tests;
