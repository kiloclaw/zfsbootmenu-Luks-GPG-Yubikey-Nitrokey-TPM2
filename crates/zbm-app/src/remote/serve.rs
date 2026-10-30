//! The session's side: it takes the menu's terminal, then serves its
//! requests, the actions that leave the console alone on a
//! [`MachineService`], those that take it on the terminal.

use std::io;
use std::os::fd::{AsFd as _, OwnedFd};
use std::time::Duration;

use zbm_sys::console::{Watch, wait_ready};
use zbm_sys::socket::Connection;

use super::{Received, ToMenu, ToSession};
use crate::model::{Action, Event, Outcome};
use crate::service::{MachineService, Report, ServiceHandle};

/// How long the menu may take to attach once the session yielded.
pub const ATTACH_WAIT: Duration = Duration::from_secs(5);

/// How often the session looks whether the menu is to end, while nothing
/// comes.
const STOP_POLL: Duration = Duration::from_millis(250);

/// Why the session ended the menu: another took it over, or the session
/// ends.
pub const TAKEN_OVER: &str = "the menu was taken over";

/// The terminal a menu attached with, its input and its output.
#[derive(Debug)]
pub struct Attached {
    /// The terminal's input.
    pub input: OwnedFd,
    /// The terminal's output.
    pub output: OwnedFd,
}

/// Waits at most [`ATTACH_WAIT`] for the menu on `connection` to attach,
/// and says it did.
///
/// # Errors
///
/// When it did not, or attached without its terminal's input and output.
pub fn attach(connection: &Connection) -> io::Result<Attached> {
    let received = ToSession::receive(connection, Some(ATTACH_WAIT))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "the menu did not attach"))?;
    let Received {
        message: ToSession::Attach,
        fds,
    } = received
    else {
        let refusal = "attach first";
        ToMenu::Refused(refusal.to_owned()).send(connection)?;
        return Err(io::Error::new(io::ErrorKind::InvalidData, refusal));
    };
    let Ok([input, output]) = <[OwnedFd; 2]>::try_from(fds) else {
        let refusal = "a terminal is its input and its output";
        ToMenu::Refused(refusal.to_owned()).send(connection)?;
        return Err(io::Error::new(io::ErrorKind::InvalidData, refusal));
    };
    ToMenu::Attached.send(connection)?;
    Ok(Attached { input, output })
}

/// What the session serves a menu with.
pub struct Served<'s, 'a> {
    /// The machine, which performs the actions that leave the console
    /// alone.
    pub service: MachineService<'a>,
    /// Performs an action that takes the console, on the menu's terminal.
    pub console: &'s (dyn Fn(&Action) -> Vec<Event> + Sync),
    /// Whether the menu is to end: another took it over, or the session
    /// ends.
    pub stop: &'s (dyn Fn() -> bool + Sync),
}

/// Serves the menu on `connection` until it detaches or goes, or `served`
/// says to stop, when it is told [`TAKEN_OVER`].
///
/// # Errors
///
/// When the menu sent what is none of the protocol, or the service could
/// not start.
pub fn serve(connection: &Connection, served: &Served<'_, '_>) -> io::Result<()> {
    std::thread::scope(|scope| {
        let mut handle = served.service.start(scope)?;
        loop {
            if (served.stop)() {
                handle.cancel();
                let _ = ToMenu::Refused(TAKEN_OVER.to_owned()).send(connection);
                return Ok(());
            }
            let watches = [Watch::read(connection.as_fd()), Watch::read(handle.as_fd())];
            wait_ready(&watches, Some(STOP_POLL))?;
            forward(connection, &mut handle)?;
            let message = match ToSession::receive(connection, Some(Duration::ZERO)) {
                Ok(Some(received)) => received.message,
                Ok(None) => continue,
                // The menu went.
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(error) => return Err(error),
            };
            if !answer(connection, served, &mut handle, message)? {
                return Ok(());
            }
        }
    })
}

/// Answers `message`; whether the menu goes on.
fn answer(
    connection: &Connection,
    served: &Served<'_, '_>,
    handle: &mut ServiceHandle,
    message: ToSession,
) -> io::Result<bool> {
    match message {
        ToSession::Attach => refuse(connection, "already attached")?,
        ToSession::Read => {
            ToMenu::Snapshot(served.service.read().map(Box::new)).send(connection)?;
        }
        ToSession::Send { action } => {
            if handle.send(action).is_err() {
                fail(connection, "the machine's service ended")?;
            }
        }
        // The terminal is the menu's again only once the action is done.
        ToSession::Console { .. } if handle.is_busy() => {
            fail(connection, "another action is being performed")?;
        }
        ToSession::Console { action } => {
            let mut events = (served.console)(&action);
            if let Some(snapshot) = served.service.read() {
                events.insert(0, Event::Changed(snapshot));
            }
            for event in events {
                ToMenu::Event(Box::new(event)).send(connection)?;
            }
            ToMenu::Done.send(connection)?;
        }
        ToSession::Cancel => handle.cancel(),
        ToSession::Detach => {
            handle.cancel();
            return Ok(false);
        }
    }
    Ok(true)
}

/// Sends the menu what the service reported.
fn forward(connection: &Connection, handle: &mut ServiceHandle) -> io::Result<()> {
    for report in handle.take() {
        match report {
            Report::Event(event) => ToMenu::Event(event).send(connection)?,
            Report::Done => ToMenu::Done.send(connection)?,
        }
    }
    Ok(())
}

/// Ends an action that cannot be performed, for `reason`: it failed.
fn fail(connection: &Connection, reason: &str) -> io::Result<()> {
    let failed = Event::Finished(Outcome::Failed {
        what: "unable to perform the action".to_owned(),
        why: vec![reason.to_owned()],
        next: Vec::new(),
    });
    ToMenu::Event(Box::new(failed)).send(connection)?;
    ToMenu::Done.send(connection)
}

/// Refuses a request, for `reason`.
fn refuse(connection: &Connection, reason: &str) -> io::Result<()> {
    ToMenu::Refused(reason.to_owned()).send(connection)
}

#[cfg(test)]
mod tests;
