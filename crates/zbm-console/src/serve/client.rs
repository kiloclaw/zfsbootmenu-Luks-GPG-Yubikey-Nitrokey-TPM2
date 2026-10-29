//! The requests to the console on its control socket, as `zbm console
//! release`, `reattach`, `layout` and `font` send them, with what their
//! replies mean to those commands.

use std::io;
use std::path::Path;
use std::time::Duration;

use zbm_app::console::{Refusal, Reply, Request, Status, Unanswered, ask};

/// What became of a request to hand the display back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Released {
    /// It was handed back, or no console held it.
    Done,
    /// The console did not hand it back in time.
    Late,
}

/// An error for a reply that does not answer `request`.
fn unexpected(request: &str, reply: &Reply) -> io::Error {
    io::Error::other(match reply {
        Reply::Refused(refusal) => format!("the console refused to {request}: {refusal}"),
        _ => format!("the console's reply does not answer {request}: {reply:?}"),
    })
}

/// An error for a request that got no reply.
fn unanswered(unanswered: Unanswered) -> io::Error {
    io::Error::other(unanswered.to_string())
}

/// `zbm console release`: asks the console listening at `socket` to hand
/// the display back, and waits at most `timeout`.
///
/// # Errors
///
/// When the console refused, or answered in no way the protocol knows.
pub fn release(socket: &Path, timeout: Duration) -> io::Result<Released> {
    match ask(socket, &Request::Release, timeout) {
        Ok(Reply::Released) | Err(Unanswered::NoConsole) => Ok(Released::Done),
        Ok(Reply::Refused(Refusal::Busy)) | Err(Unanswered::Late) => Ok(Released::Late),
        Ok(reply) => Err(unexpected("release", &reply)),
        Err(other) => Err(unanswered(other)),
    }
}

/// `zbm console reattach`, after a failed `kexec`: asks the console
/// listening at `socket`, which handed the display back, to take it again,
/// and waits at most `timeout` until it did. Returns whether it did;
/// without such a console there is nothing to do, and it is `true`.
///
/// # Errors
///
/// When the console answered in no way the protocol knows.
pub fn reattach(socket: &Path, timeout: Duration) -> io::Result<bool> {
    match ask(socket, &Request::Reattach, timeout) {
        Ok(Reply::Reattached) | Err(Unanswered::NoConsole) => Ok(true),
        Ok(Reply::Refused(Refusal::Busy | Refusal::NotShown)) | Err(Unanswered::Late) => Ok(false),
        Ok(reply) => Err(unexpected("reattach", &reply)),
        Err(other) => Err(unanswered(other)),
    }
}

/// What became of a request for a keyboard layout, or a font's size.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Switched {
    /// The console took it, and types with this layout, or draws with this
    /// font, now.
    Done(String),
    /// The image has no layout of that name, or it did not load: the
    /// keyboards keep theirs.
    NoSuchLayout,
    /// No console reads the keyboards, or draws on the display.
    NoConsole,
    /// The console did not take it in time.
    Late,
}

/// The [`Switched`] of a reply to a request for a layout or a font.
fn switched(reply: Result<Reply, Unanswered>, request: &str) -> io::Result<Switched> {
    match reply {
        Ok(Reply::LayoutSet(name) | Reply::FontSet(name)) => Ok(Switched::Done(name)),
        Ok(Reply::NoSuchLayout(_)) => Ok(Switched::NoSuchLayout),
        Ok(Reply::Refused(Refusal::NotShown)) | Err(Unanswered::NoConsole) => {
            Ok(Switched::NoConsole)
        }
        Ok(Reply::Refused(Refusal::Busy)) | Err(Unanswered::Late) => Ok(Switched::Late),
        Ok(reply) => Err(unexpected(request, &reply)),
        Err(other) => Err(unanswered(other)),
    }
}

/// `zbm console layout <name>`: asks the console listening at `socket`
/// for the keyboard layout `name`, and waits at most `timeout` until it
/// took it.
///
/// # Errors
///
/// When the console refused, or answered in no way the protocol knows.
pub fn request_layout(socket: &Path, name: &str, timeout: Duration) -> io::Result<Switched> {
    let reply = ask(socket, &Request::Layout(name.to_owned()), timeout);
    switched(reply, "switch layouts")
}

/// `zbm console font <size>`: asks the console listening at `socket` for
/// the font's size `size` (`+`, `-`, `default` or a height in pixels), and
/// waits at most `timeout` until it took the request; the font it draws
/// with then is in [`Switched::Done`].
///
/// # Errors
///
/// When the console refused, or answered in no way the protocol knows.
pub fn request_font(socket: &Path, size: &str, timeout: Duration) -> io::Result<Switched> {
    let reply = ask(socket, &Request::Font(size.to_owned()), timeout);
    switched(reply, "change the font")
}

/// `zbm console layout` and `font` without arguments: how the console
/// listening at `socket` is; `None` when none listens, or it did not
/// answer in `timeout`.
pub fn status(socket: &Path, timeout: Duration) -> Option<Status> {
    match ask(socket, &Request::Status, timeout) {
        Ok(Reply::Status(status)) => Some(status),
        _ => None,
    }
}
