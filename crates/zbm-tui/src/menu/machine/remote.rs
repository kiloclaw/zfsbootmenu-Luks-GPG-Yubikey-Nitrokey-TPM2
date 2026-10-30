//! The machine of a menu that is a client of the session (ROADMAP.md,
//! PM10): `zfsbootmenu` over SSH reads it and has its actions performed by
//! the session, over the session's socket (`zbm_app::remote`). The actions
//! that take the console run in the session, on the terminal this menu
//! handed it as it attached; the others run there meanwhile, their events
//! read as they come.

use std::io;
use std::os::fd::{AsFd as _, BorrowedFd};
use std::time::Duration;

use zbm_app::remote::{ATTACH_WAIT, ToMenu, ToSession};
use zbm_sys::socket::Connection;

use super::Machine;
use crate::menu::world::{Action, Event, Snapshot, Timeout};

/// How often a wait for the session looks whether the menu is to end.
const STOP_POLL: Duration = Duration::from_millis(250);

/// The machine, through the session.
pub struct RemoteMachine<'s> {
    session: Connection,
    snapshot: Snapshot,
    /// The first snapshot went to the menu.
    sent: bool,
    /// The actions sent that are not done.
    sent_actions: usize,
    /// Whether the menu should end: its terminal went, or another
    /// instance asked it to.
    stop: &'s dyn Fn() -> bool,
}

impl<'s> RemoteMachine<'s> {
    /// Attaches to the session on `session` with the terminal read on
    /// `input` and written on `output`, and waits for the machine: the
    /// session runs the menu program on that terminal first, whose prompts
    /// wait for the user, until `stop` says to end. The menu then waits for
    /// the user, without the countdown.
    ///
    /// # Errors
    ///
    /// When the session refused the menu, its reason; when it went, or
    /// could not read the machine; when `stop` said to end.
    pub fn attach(
        session: Connection,
        (input, output): (BorrowedFd<'_>, BorrowedFd<'_>),
        stop: &'s dyn Fn() -> bool,
    ) -> io::Result<RemoteMachine<'s>> {
        ToSession::Attach.send(&session, &[input, output])?;
        match ToMenu::receive(&session, Some(ATTACH_WAIT))? {
            Some(ToMenu::Attached) => {}
            other => return Err(unexpected(other)),
        }
        // A session that ended the menu program at once went already: what
        // it said before it went comes all the same.
        let _ = ToSession::Read.send(&session, &[]);
        let snapshot = loop {
            if stop() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "the menu ended"));
            }
            match ToMenu::receive(&session, Some(STOP_POLL))? {
                None => {}
                Some(ToMenu::Snapshot(Some(snapshot))) => break *snapshot,
                Some(ToMenu::Snapshot(None)) => {
                    return Err(io::Error::other("the machine could not be read"));
                }
                other => return Err(unexpected(other)),
            }
        };
        let mut machine = RemoteMachine {
            session,
            snapshot,
            sent: false,
            sent_actions: 0,
            stop,
        };
        machine.snapshot.timeout = Timeout::Wait;
        Ok(machine)
    }

    /// Keeps a snapshot the session sent: it waits for the user, as the
    /// countdown does not run again.
    fn keep(&mut self, event: &mut Event) {
        if let Event::Changed(snapshot) = event {
            snapshot.timeout = Timeout::Wait;
            self.snapshot = snapshot.clone();
        }
    }

    /// What `message` means to the menu, if anything; whether an action
    /// it sent is done.
    fn read(&mut self, message: ToMenu) -> (Option<Event>, bool) {
        match message {
            ToMenu::Event(event) => {
                let mut event = *event;
                self.keep(&mut event);
                (Some(event), false)
            }
            ToMenu::Done => (None, true),
            // Another menu took the session's over, the session ends, or it
            // serves this menu no more.
            ToMenu::Refused(_) => (Some(Event::Leaving(String::new())), false),
            ToMenu::Attached | ToMenu::Snapshot(_) => (None, false),
        }
    }

    /// What the session sent until now.
    fn received(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            match ToMenu::receive(&self.session, Some(Duration::ZERO)) {
                Ok(Some(message)) => {
                    let (event, done) = self.read(message);
                    if done {
                        self.sent_actions = self.sent_actions.saturating_sub(1);
                    }
                    // Nothing after the end matters.
                    if let Some(leaving @ Event::Leaving(_)) = event {
                        events.push(leaving);
                        return events;
                    }
                    events.extend(event);
                }
                Ok(None) => break,
                Err(_) => {
                    events.push(Event::Leaving(String::new()));
                    return events;
                }
            }
        }
        if (self.stop)() {
            events.push(Event::Leaving(String::new()));
        }
        events
    }

    /// Performs `action` in the session, on this menu's terminal: what
    /// happened, once it is done.
    fn on_console(&mut self, action: Action) -> Vec<Event> {
        let message = ToSession::Console { action };
        if message.send(&self.session, &[]).is_err() {
            return vec![Event::Leaving(String::new())];
        }
        let mut events = Vec::new();
        loop {
            match ToMenu::receive(&self.session, None) {
                Ok(Some(message)) => {
                    let (event, done) = self.read(message);
                    events.extend(event);
                    if done {
                        return events;
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    events.push(Event::Leaving(String::new()));
                    return events;
                }
            }
        }
    }
}

/// The error of a message the session should not have sent then.
fn unexpected(message: Option<ToMenu>) -> io::Error {
    match message {
        Some(ToMenu::Refused(reason)) => io::Error::other(reason),
        None => io::Error::new(io::ErrorKind::TimedOut, "the session did not answer"),
        Some(other) => io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the session sent {other:?}"),
        ),
    }
}

impl Machine for RemoteMachine<'_> {
    fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The first snapshot, which opens the main view; then what came.
    fn tick(&mut self) -> Vec<Event> {
        if self.sent {
            return self.received();
        }
        self.sent = true;
        vec![Event::Changed(self.snapshot.clone())]
    }

    fn poll(&mut self) -> Vec<Event> {
        self.received()
    }

    /// Performs `action` in the session: one that takes the console on
    /// this menu's terminal, at once; the others meanwhile. The recovery
    /// shell ends the menu, as the menu run in its own process does.
    fn apply(&mut self, action: Action) -> Vec<Event> {
        if action == Action::Shell {
            return vec![Event::Leaving(String::new())];
        }
        if action.needs_console_on(&self.snapshot) {
            return self.on_console(action);
        }
        let message = ToSession::Send { action };
        if message.send(&self.session, &[]).is_err() {
            return vec![Event::Leaving(String::new())];
        }
        self.sent_actions += 1;
        Vec::new()
    }

    fn needs_console(&self, action: &Action) -> bool {
        action.needs_console_on(&self.snapshot) && *action != Action::Shell
    }

    fn is_busy(&self) -> bool {
        self.sent_actions > 0
    }

    fn cancel(&mut self) {
        let _ = ToSession::Cancel.send(&self.session, &[]);
    }

    fn waker(&self) -> Option<BorrowedFd<'_>> {
        Some(self.session.as_fd())
    }
}

impl Drop for RemoteMachine<'_> {
    /// The menu ends: the session's menu program ends too.
    fn drop(&mut self) {
        let _ = ToSession::Detach.send(&self.session, &[]);
    }
}

#[cfg(test)]
mod tests;
