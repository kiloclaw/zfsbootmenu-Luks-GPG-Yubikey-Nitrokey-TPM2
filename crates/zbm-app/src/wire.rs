//! What the sockets between the zones share (ADR 0002): messages as JSON
//! objects that carry the version of their protocol, a listener that
//! hands each peer on until it is told to stop, and the check of who the
//! peer is.

use std::os::fd::AsFd as _;
use std::time::Duration;

use serde_json::{Map, Value};
use zbm_platform::event::CancelToken;
use zbm_sys::console::{Watch, wait_ready};
use zbm_sys::socket::{Connection, Listener};

/// How long a socket waits for a peer's request once it connected.
pub const REQUEST_WAIT: Duration = Duration::from_secs(1);

/// The object of a message, with the version it carries.
pub(crate) fn message(bytes: &[u8]) -> Option<(u64, Map<String, Value>)> {
    let Ok(Value::Object(object)) = serde_json::from_slice(bytes) else {
        return None;
    };
    let version = object.get("version")?.as_u64()?;
    Some((version, object))
}

/// The text of `key` in `object`.
pub(crate) fn text<'o>(object: &'o Map<String, Value>, key: &str) -> Option<&'o str> {
    object.get(key)?.as_str()
}

/// Why a peer's request is not read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unread {
    /// The peer is not of the user it must be.
    Stranger,
    /// It sent nothing in time, or the socket failed.
    Nothing,
}

/// The request `peer` sent, if `peer` is of `user`. It is read even from a
/// stranger: a peer whose request is left unread gets no refusal, but a
/// reset.
///
/// # Errors
///
/// When `peer` is a stranger, or sent nothing in [`REQUEST_WAIT`].
pub fn request_of(peer: &Connection, user: u32) -> Result<Vec<u8>, Unread> {
    let request = peer.receive(Some(REQUEST_WAIT));
    let trusted = peer.peer().is_ok_and(|credentials| credentials.uid == user);
    if !trusted {
        return Err(Unread::Stranger);
    }
    match request {
        Ok(Some(bytes)) => Ok(bytes),
        _ => Err(Unread::Nothing),
    }
}

/// Gives each peer that connects to `listener` to `each`, until `until` is
/// cancelled or the socket fails; `name` says whose socket it is in the
/// log: the session's, the console's.
pub fn accept_until(
    listener: &Listener,
    until: &CancelToken,
    name: &str,
    mut each: impl FnMut(Connection),
) {
    loop {
        let watches = [Watch::read(listener.as_fd()), Watch::read(until.as_fd())];
        let ready = wait_ready(&watches, None);
        if until.is_cancelled() {
            return;
        }
        // A peer that went before it was accepted is none.
        match ready.and_then(|_| listener.accept(Some(Duration::ZERO))) {
            Ok(peer) => peer.into_iter().for_each(&mut each),
            Err(error) => {
                tracing::error!(%error, socket = name, "a socket failed");
                return;
            }
        }
    }
}
