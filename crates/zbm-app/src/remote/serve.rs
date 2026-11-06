//! The session's side: it takes the menu's terminal, then serves its
//! requests, the actions that leave the console alone on a
//! [`MachineService`], those that take it on the terminal. What an action
//! asks goes to the menu, whose answer goes to the question's reply. The
//! session opens only a volume of the machine as it reads it, as the menu
//! names it, and none in the ways of `zluks` alone; it reads the kernel's
//! messages only about a device of a pool it shows.

use std::io;
use std::os::fd::{AsFd as _, OwnedFd};
use std::time::Duration;

use zbm_platform::event::Reply;
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
        // What the action performed asks now.
        let mut asked = Asked::default();
        loop {
            if (served.stop)() {
                handle.cancel();
                let _ = ToMenu::Refused(TAKEN_OVER.to_owned()).send(connection);
                return Ok(());
            }
            let watches = [Watch::read(connection.as_fd()), Watch::read(handle.as_fd())];
            wait_ready(&watches, Some(STOP_POLL))?;
            match forward(connection, &mut handle, &mut asked) {
                // The menu went while the service reported.
                Err(error) if is_gone(&error) => return Ok(()),
                forwarded => forwarded?,
            }
            let message = match ToSession::receive(connection, Some(Duration::ZERO)) {
                Ok(Some(received)) => received.message,
                Ok(None) => continue,
                // The menu went.
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(error) => return Err(error),
            };
            if let ToSession::Answer { question, action } = message {
                asked.answer(question, action);
                continue;
            }
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
        ToSession::Send { action } | ToSession::Console { action }
            if let Some(reason) = refusal(served, &action) =>
        {
            fail(connection, &reason)?;
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
        // Answered as it comes, before this.
        ToSession::Answer { .. } => {}
        ToSession::Detach => {
            handle.cancel();
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `error`, of a send, says the menu's end of the connection is
/// closed.
fn is_gone(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
    )
}

/// Why the session does not perform `action` for a remote menu, if it
/// does not: an answer goes as one ([`ToSession::Answer`]); the menu opens
/// volumes one at a time, with their key methods, and only those the
/// machine has, named as the menu names them; reads the messages of the
/// disks of the pools it shows alone, and the snapshots of the machine's
/// boot environments alone; the boot history, which takes nothing from the
/// menu, is read as on the console. A change such as an export is the
/// service's to refuse, as the lock keeps it.
fn refusal(served: &Served<'_, '_>, action: &Action) -> Option<String> {
    match action {
        Action::Unlock { .. } | Action::SkipUnlock(_) | Action::Answer(_) => {
            Some("an answer goes with the question it answers".to_owned())
        }
        Action::UnlockAll | Action::OpenWithKeyFile { .. } | Action::Close(_) => {
            Some("the menu opens one volume at a time".to_owned())
        }
        Action::Open { device, name } => {
            // The volumes alone: the pools and kernels are not read, so that
            // what the action performed reports meanwhile goes on at once.
            let volumes = served.service.volumes();
            let known = volumes.iter().any(|volume| {
                volume.device.get() == device
                    && name
                        .as_ref()
                        .is_none_or(|name| name.as_str() == volume.name.get())
            });
            (!known).then(|| "the machine has no such volume".to_owned())
        }
        Action::LoadDeviceLog(device) => {
            let pools = served.service.pools();
            let known = pools.iter().any(|pool| pool.has_disk(device.get()));
            (!known).then(|| "no pool of the machine has such a device".to_owned())
        }
        Action::LoadSnapshots(environment) => (!served.service.is_environment(environment))
            .then(|| "the machine has no such boot environment".to_owned()),
        _ => None,
    }
}

/// What the action performed asks the menu now: the number of its
/// question, and the reply its answer goes to.
#[derive(Default)]
struct Asked {
    /// The number of the question asked last; the next is one more.
    last: u64,
    /// The reply of question `last`, while it is not answered or withdrawn.
    reply: Option<Reply<Action>>,
}

impl Asked {
    /// Asks the next question, whose answer goes to `reply`: its number.
    fn ask(&mut self, reply: Reply<Action>) -> u64 {
        self.last += 1;
        self.reply = Some(reply);
        self.last
    }

    /// The question asked now ends unanswered.
    fn withdraw(&mut self) {
        self.reply = None;
    }

    /// Sends `action` to the reply of question `question`, if it is an
    /// answer and that question is the one asked now: an answer to a
    /// question withdrawn meanwhile, or to nothing, goes to nobody.
    fn answer(&mut self, question: u64, action: Action) {
        if action.is_answer()
            && question == self.last
            && let Some(reply) = self.reply.take()
        {
            reply.send(action);
        }
    }
}

/// Sends the menu what the service reported, keeping in `asked` what its
/// action asks.
fn forward(
    connection: &Connection,
    handle: &mut ServiceHandle,
    asked: &mut Asked,
) -> io::Result<()> {
    for report in handle.take() {
        match report {
            Report::Event(event) => ToMenu::Event(event).send(connection)?,
            Report::Ask { event, reply } => {
                let question = asked.ask(reply);
                ToMenu::Ask { question, event }.send(connection)?;
            }
            Report::Withdraw => {
                asked.withdraw();
                ToMenu::Event(Box::new(Event::Withdrawn)).send(connection)?;
            }
            Report::Done => {
                asked.withdraw();
                ToMenu::Done.send(connection)?;
            }
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
