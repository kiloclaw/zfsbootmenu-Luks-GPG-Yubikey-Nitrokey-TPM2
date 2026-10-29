//! The machine as a service (ADR 0001, PM8): a worker that owns the
//! [`App`], performs the actions a front end sends it one after another,
//! and sends back what happened, so that the front end goes on drawing,
//! counting down and taking keys meanwhile.
//!
//! [`MachineService::start`] starts the worker in a scope, and gives the
//! front end a [`ServiceHandle`]: [`ServiceHandle::send`] queues an action,
//! [`ServiceHandle::take`] collects what came back, and the handle is
//! readable (its `eventfd`) when something did, for the front end's `poll`.
//! [`ServiceHandle::cancel`] cancels what was sent: the program the action
//! waits for is killed, nothing more starts, and the action ends as
//! [`Outcome::Cancelled`].
//!
//! Actions that take the console stay with the front end, which gives the
//! console to them ([`App::act_on_console`]); the service performs the
//! others, as [`App::act_quietly`] does.

use std::collections::VecDeque;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::Scope;

use zbm_platform::event::{CancelToken, MailSender, Mailbox, spawn_scoped_traced};
use zbm_platform::{CancellingRunner, System};

use crate::App;
use crate::model::{Action, Event, Outcome, Snapshot};

/// What the service sends back.
#[derive(Debug)]
pub enum Report {
    /// Something happened while performing the action; boxed, as a
    /// snapshot is large.
    Event(Box<Event>),
    /// The action ended; its last event said how.
    Done,
}

/// An action, with what cancels it.
struct Request {
    action: Action,
    token: CancelToken,
}

/// The machine as a service: the application, and the front end's whole
/// system, whose console the service leaves alone.
#[derive(Clone, Copy)]
pub struct MachineService<'a> {
    app: App<'a>,
    console: System<'a>,
}

impl<'a> MachineService<'a> {
    /// The service of `app`, on `console`, the front end's whole system.
    pub fn new(app: App<'a>, console: System<'a>) -> MachineService<'a> {
        MachineService { app, console }
    }

    /// Performs `action`, its programs cancelled by `token`, then reads the
    /// machine again, as the action may have changed it: the new snapshot
    /// comes first, the action's events after it, the last saying how it
    /// ended. A cancelled action ends as [`Outcome::Cancelled`].
    pub fn perform(&self, action: &Action, token: &CancelToken) -> Vec<Event> {
        let runner = CancellingRunner::new(self.app.runner(), token.clone());
        let app = self.app.with_runner(&runner);
        let mut events = if token.is_cancelled() {
            Vec::new()
        } else {
            app.act_quietly(action, self.console)
        };
        if token.is_cancelled() {
            if matches!(events.last(), Some(Event::Finished(_))) {
                events.pop();
            }
            events.push(Event::Finished(Outcome::Cancelled));
        }
        if let Some(snapshot) = self.read() {
            events.insert(0, Event::Changed(snapshot));
        }
        events
    }

    /// The machine as the menu shows it, with the kernels of the
    /// environments whose keys are loaded; `None` when it cannot be read.
    pub fn read(&self) -> Option<Snapshot> {
        let mut snapshot = self.app.snapshot().ok()?;
        self.app.read_kernels(&mut snapshot, self.console);
        Some(snapshot)
    }

    /// Starts the service on a worker in `scope`, which ends when the handle
    /// is dropped and the action it performs ended.
    ///
    /// # Errors
    ///
    /// When the mailbox cannot be made.
    pub fn start<'scope>(self, scope: &'scope Scope<'scope, '_>) -> io::Result<ServiceHandle>
    where
        'a: 'scope,
    {
        let reports = Mailbox::new()?;
        let (requests, inbox) = channel();
        let sender = reports.sender();
        spawn_scoped_traced(scope, move || self.serve(&inbox, &sender));
        Ok(ServiceHandle {
            requests,
            reports,
            sent: VecDeque::new(),
        })
    }

    /// Performs the requests of `inbox` until the handle is gone.
    fn serve(&self, inbox: &Receiver<Request>, reports: &MailSender<Report>) {
        let _span = tracing::info_span!("service").entered();
        while let Ok(request) = inbox.recv() {
            for event in self.perform(&request.action, &request.token) {
                if reports.send(Report::Event(Box::new(event))).is_err() {
                    return;
                }
            }
            if reports.send(Report::Done).is_err() {
                return;
            }
        }
    }
}

/// The front end's side of a [`MachineService`].
pub struct ServiceHandle {
    requests: Sender<Request>,
    reports: Mailbox<Report>,
    /// The tokens of the actions sent and not done, the one performed first.
    sent: VecDeque<CancelToken>,
}

impl ServiceHandle {
    /// Queues `action`.
    ///
    /// # Errors
    ///
    /// When the token cannot be made, or the service is gone: the action
    /// was not sent.
    pub fn send(&mut self, action: Action) -> io::Result<()> {
        let token = CancelToken::new()?;
        let request = Request {
            action,
            token: token.clone(),
        };
        self.requests
            .send(request)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "the service ended"))?;
        self.sent.push_back(token);
        Ok(())
    }

    /// Cancels every action sent that is not done.
    pub fn cancel(&self) {
        for token in &self.sent {
            token.cancel();
        }
    }

    /// Whether an action sent is not done.
    pub fn is_busy(&self) -> bool {
        !self.sent.is_empty()
    }

    /// What came back since the last time, in order.
    pub fn take(&mut self) -> Vec<Report> {
        let reports = self.reports.take();
        for report in &reports {
            if matches!(report, Report::Done) {
                self.sent.pop_front();
            }
        }
        reports
    }
}

impl Drop for ServiceHandle {
    /// The front end is gone: what it sent is cancelled, so that the
    /// worker, which its scope waits for, ends soon.
    fn drop(&mut self) {
        self.cancel();
    }
}

impl AsFd for ServiceHandle {
    /// Readable when something came back.
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.reports.as_fd()
    }
}

#[cfg(test)]
mod tests;
