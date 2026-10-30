//! A menu as a client of the session (ROADMAP.md, PM10; ADR 0002): the
//! messages between a menu in a process of its own, as over SSH, and the
//! session that owns the machine. The menu reads the machine's snapshot,
//! sends actions and is told what happened, as the menu in the session
//! asks its `MachineService`; an action that takes the console runs on the
//! terminal the menu handed the session as it attached.
//!
//! The messages go on the session's socket (`zbm_app::session`), after the
//! session yielded its own menu: JSON with the [`VERSION`] of this
//! protocol, the vocabulary of `zbm_app::model` as it is. A message longer
//! than a packet is cut in parts, each packet's first byte saying whether
//! more follow (`+`) or it is the last (`.`), up to [`MAX_LENGTH`] in all.
//! A secret is never in a message: its length is, and its bytes follow as
//! a packet of their own, from and into memory that is wiped. A name read
//! from a message is checked by its parser, as any other input is.
//!
//! ```text
//! menu                               session
//! .{"version":1,"message":"attach"}  + the terminal's input and output
//!                                    .{"version":1,"message":"attached"}
//! .{"version":1,"message":"read"}
//!                                    +{"version":1,"message":{"snapshot"…
//!                                    .…}}
//! .{"version":1,"message":{"send":{"action":…}},"secret":9}
//! (9 bytes)
//!                                    .{"version":1,"message":{"event":…}}
//!                                    .{"version":1,"message":"done"}
//! ```

use std::io;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use zbm_platform::Secret;
use zbm_sys::socket::{Connection, MAX_MESSAGE};

use crate::model::{Action, Event, Snapshot};

/// The version of the protocol.
pub const VERSION: u64 = 1;

/// The longest message, all its parts together: a snapshot of many
/// thousands of snapshots fits; a peer cannot make the other side keep
/// more.
pub const MAX_LENGTH: usize = 16 * 1024 * 1024;

/// How long the parts after the first may take to come.
const PART_WAIT: Duration = Duration::from_secs(5);

/// What a menu asks the session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToSession {
    /// The menu attaches, with its terminal's input and output, on which
    /// the actions that take the console run.
    Attach,
    /// The machine as it is now, its kernels read.
    Read,
    /// An action that leaves the console alone, performed meanwhile: its
    /// events come as [`ToMenu::Event`], then [`ToMenu::Done`].
    Send {
        /// The action.
        action: Action,
    },
    /// An action that takes the console, performed on the menu's
    /// terminal: its events, then [`ToMenu::Done`], once it returned.
    Console {
        /// The action.
        action: Action,
    },
    /// Cancels the actions sent and not done.
    Cancel,
    /// The menu ends.
    Detach,
}

/// What the session tells a menu.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToMenu {
    /// The menu is attached.
    Attached,
    /// The machine; `None` when it could not be read.
    Snapshot(Option<Box<Snapshot>>),
    /// Something happened while an action was performed.
    Event(Box<Event>),
    /// The action ended; its last event said how.
    Done,
    /// The request was refused, for this reason.
    Refused(String),
}

/// A message on the wire.
#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    version: u64,
    message: T,
    /// The length of the secret that follows, if one does: an empty one
    /// follows as no packet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secret: Option<usize>,
}

/// A message received, with the descriptors it carried.
#[derive(Debug)]
pub struct Received<T> {
    /// The message.
    pub message: T,
    /// The descriptors that came with it: a terminal's, with
    /// [`ToSession::Attach`].
    pub fds: Vec<OwnedFd>,
}

/// The first byte of a part that more follow, and of the last.
const MORE: u8 = b'+';
const LAST: u8 = b'.';

/// The secret of `action`, an unlocking one, taken out of it: it does not
/// cross in the message.
fn take_secret(action: &mut Action) -> Option<Arc<Secret>> {
    match action {
        Action::Unlock { secret, .. } => secret.take(),
        Action::Plan(inner) => take_secret(inner),
        _ => None,
    }
}

/// Puts `secret` back into `action`.
fn give_secret(action: &mut Action, given: Arc<Secret>) {
    match action {
        Action::Unlock { secret, .. } => *secret = Some(given),
        Action::Plan(inner) => give_secret(inner, given),
        _ => {}
    }
}

/// Sends `bytes` in parts, `fds` with the first.
fn send_parts(connection: &Connection, bytes: &[u8], fds: &[BorrowedFd<'_>]) -> io::Result<()> {
    if bytes.len() > MAX_LENGTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("a message of {} bytes", bytes.len()),
        ));
    }
    let mut parts = bytes.chunks(MAX_MESSAGE - 1).peekable();
    let mut first = true;
    // An empty message is one last part.
    let mut sent = false;
    while let Some(part) = parts.next() {
        let mut packet = Vec::with_capacity(part.len() + 1);
        packet.push(if parts.peek().is_some() { MORE } else { LAST });
        packet.extend_from_slice(part);
        connection.send_with(&packet, if first { fds } else { &[] })?;
        first = false;
        sent = true;
    }
    if !sent {
        connection.send_with(&[LAST], fds)?;
    }
    Ok(())
}

/// The parts of the next message, joined, and the descriptors of the
/// first; `None` when none came in `timeout`.
fn receive_parts(
    connection: &Connection,
    timeout: Option<Duration>,
) -> io::Result<Option<(Vec<u8>, Vec<OwnedFd>)>> {
    let Some((first, fds)) = connection.receive_with(timeout)? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    let mut packet = first;
    loop {
        let Some((&kind, part)) = packet.split_first() else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the peer is gone",
            ));
        };
        if bytes.len() + part.len() > MAX_LENGTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a message too long",
            ));
        }
        bytes.extend_from_slice(part);
        match kind {
            LAST => return Ok(Some((bytes, fds))),
            MORE => {}
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "not a part")),
        }
        packet = connection
            .receive(Some(PART_WAIT))?
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "a part that did not come"))?;
    }
}

/// The secret of `length` bytes that follows a message on `connection`.
fn receive_secret(connection: &Connection, length: usize) -> io::Result<Vec<u8>> {
    if length == 0 {
        return Ok(Vec::new());
    }
    let mut bytes = connection
        .receive(Some(PART_WAIT))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "no secret came"))?;
    if bytes.len() == length {
        Ok(bytes)
    } else {
        zbm_sys::wipe_vec(&mut bytes);
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a secret cut short",
        ))
    }
}

/// `message` in its envelope, announcing a secret of `secret` bytes.
fn encode<T: Serialize>(message: &T, secret: Option<usize>) -> io::Result<Vec<u8>> {
    let envelope = Envelope {
        version: VERSION,
        message,
        secret,
    };
    serde_json::to_vec(&envelope).map_err(io::Error::other)
}

/// The message in `bytes`, of this version.
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> io::Result<Envelope<T>> {
    let envelope: Envelope<T> = serde_json::from_slice(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if envelope.version == VERSION {
        Ok(envelope)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("version {} of the protocol", envelope.version),
        ))
    }
}

impl ToSession {
    /// The action it carries, if it carries one.
    fn action_mut(&mut self) -> Option<&mut Action> {
        match self {
            ToSession::Send { action } | ToSession::Console { action } => Some(action),
            _ => None,
        }
    }

    /// Sends it on `connection`, with `fds`; an unlocking action's secret
    /// follows as a packet of its own.
    ///
    /// # Errors
    ///
    /// When the session is gone.
    pub fn send(mut self, connection: &Connection, fds: &[BorrowedFd<'_>]) -> io::Result<()> {
        let secret = self.action_mut().and_then(take_secret);
        let length = secret.as_ref().map(|secret| secret.len());
        send_parts(connection, &encode(&self, length)?, fds)?;
        if let Some(secret) = secret.filter(|secret| !secret.is_empty()) {
            connection.send(secret.expose())?;
        }
        Ok(())
    }

    /// The next message a menu sent on `connection`, waiting at most
    /// `timeout`; `None` when none came in time.
    ///
    /// # Errors
    ///
    /// When the menu is gone, or sent none of the protocol.
    pub fn receive(
        connection: &Connection,
        timeout: Option<Duration>,
    ) -> io::Result<Option<Received<ToSession>>> {
        let Some((bytes, fds)) = receive_parts(connection, timeout)? else {
            return Ok(None);
        };
        Self::read(&bytes, fds, connection).map(Some)
    }

    /// The message in `bytes`, with its secret from `connection`.
    fn read(
        bytes: &[u8],
        fds: Vec<OwnedFd>,
        connection: &Connection,
    ) -> io::Result<Received<ToSession>> {
        let Envelope {
            mut message,
            secret: length,
            ..
        } = decode::<ToSession>(bytes)?;
        if let Some(length) = length {
            let secret = Arc::new(Secret::from_vec(receive_secret(connection, length)?));
            if let Some(action) = message.action_mut() {
                give_secret(action, secret);
            }
        }
        Ok(Received { message, fds })
    }

    /// The message as one packet, without its secret, for the fuzz target.
    ///
    /// # Errors
    ///
    /// When it cannot be encoded.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut bytes = vec![LAST];
        bytes.extend(encode(self, None)?);
        Ok(bytes)
    }

    /// The message in `bytes`, alone, for the fuzz target: a secret it
    /// announces is not read.
    ///
    /// # Errors
    ///
    /// When it is none of the protocol.
    pub fn decode(bytes: &[u8]) -> io::Result<ToSession> {
        let Some((&LAST, json)) = bytes.split_first() else {
            return Err(io::ErrorKind::InvalidData.into());
        };
        decode::<ToSession>(json).map(|envelope| envelope.message)
    }
}

impl ToMenu {
    /// Sends it on `connection`.
    ///
    /// # Errors
    ///
    /// When the menu is gone.
    pub fn send(&self, connection: &Connection) -> io::Result<()> {
        send_parts(connection, &encode(self, None)?, &[])
    }

    /// The next message the session sent on `connection`, waiting at most
    /// `timeout`; `None` when none came in time.
    ///
    /// # Errors
    ///
    /// When the session is gone, or sent none of the protocol.
    pub fn receive(
        connection: &Connection,
        timeout: Option<Duration>,
    ) -> io::Result<Option<ToMenu>> {
        let Some((bytes, _)) = receive_parts(connection, timeout)? else {
            return Ok(None);
        };
        decode::<ToMenu>(&bytes).map(|envelope| Some(envelope.message))
    }

    /// The message in `bytes`, one packet, for the fuzz target.
    ///
    /// # Errors
    ///
    /// When it is none of the protocol.
    pub fn decode(bytes: &[u8]) -> io::Result<ToMenu> {
        let Some((&LAST, json)) = bytes.split_first() else {
            return Err(io::ErrorKind::InvalidData.into());
        };
        decode::<ToMenu>(json).map(|envelope| envelope.message)
    }

    /// The message as one packet, for the fuzz target.
    ///
    /// # Errors
    ///
    /// When it cannot be encoded.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut bytes = vec![LAST];
        bytes.extend(encode(self, None)?);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests;
