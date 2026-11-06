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
//! console to them ([`Through::Console`]); the service applies the others
//! [`Through::Quietly`]. What an action asks, as opening a volume asks for
//! its secret, comes with a reply the front end answers directly
//! ([`Report::Ask`], [`asker`]).

pub mod asker;

use std::collections::VecDeque;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::Scope;

use zbm_platform::event::{CancelToken, MailSender, Mailbox, Reply, spawn_scoped_traced};
use zbm_platform::{CancellingRunner, Runner, System};

use self::asker::Asker;
use crate::model::{Action, Event, Outcome, Pool, Snapshot, UnlockOutcome, Volume};
use crate::{App, Application as _, Through};

/// What the service sends back.
#[derive(Debug)]
pub enum Report {
    /// Something happened while performing the action; boxed, as a
    /// snapshot is large.
    Event(Box<Event>),
    /// The action asks: the dialog `event` opens, whose answer, an
    /// [`Action::is_answer`], goes to `reply`, not to the service, whose
    /// worker waits for it.
    Ask {
        /// What is asked.
        event: Box<Event>,
        /// Where the answer goes.
        reply: Reply<Action>,
    },
    /// The question asked last was answered otherwise, as by the device it
    /// waited for, or ended with its action: its dialog closes.
    Withdraw,
    /// The action ended; its last event said how.
    Done,
}

/// An action, with what cancels it.
struct Request {
    action: Action,
    token: CancelToken,
}

/// The machine as a service: the application, the front end's whole
/// system, whose console the service leaves alone, and what asks the front
/// end what the application's LUKS session asks.
#[derive(Clone, Copy)]
pub struct MachineService<'a> {
    app: App<'a>,
    console: System<'a>,
    asker: Option<&'a Asker<'a>>,
}

impl<'a> MachineService<'a> {
    /// The service of `app`, on `console`, the front end's whole system.
    pub fn new(app: App<'a>, console: System<'a>) -> MachineService<'a> {
        MachineService {
            app,
            console,
            asker: None,
        }
    }

    /// The same service, whose worker asks the front end through `asker`,
    /// the interaction of the application's LUKS session.
    #[must_use]
    pub fn with_asker(self, asker: &'a Asker<'a>) -> MachineService<'a> {
        MachineService {
            asker: Some(asker),
            ..self
        }
    }

    /// Performs `action`, its programs cancelled by `token`, then reads the
    /// machine again, as the action may have changed it, unless it only
    /// read it ([`Action::is_read`]): the new snapshot comes first, the
    /// action's events after it, the last saying how it ended. A cancelled
    /// action ends as [`Outcome::Cancelled`].
    pub fn perform(&self, action: &Action, token: &CancelToken) -> Vec<Event> {
        let runner = CancellingRunner::new(self.app.runner(), token.clone());
        let luks_runner = self
            .app
            .luks_runner()
            .map(|inner| CancellingRunner::new(inner, token.clone()));
        let luks_runner = luks_runner.as_ref().map(|runner| runner as &dyn Runner);
        let app = self.app.cancelled_by(token, &runner, luks_runner);
        let mut events = if token.is_cancelled() {
            Vec::new()
        } else {
            app.apply(action, Through::Quietly(self.console))
        };
        // A volume opened meanwhile stays open, and says so.
        if token.is_cancelled() && !has_opened(&events) {
            if matches!(events.last(), Some(Event::Finished(_))) {
                events.pop();
            }
            events.push(Event::Finished(Outcome::Cancelled));
        }
        if !action.is_read()
            && let Some(snapshot) = self.read()
        {
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

    /// The machine's LUKS volumes as the menu shows them, read alone.
    pub(crate) fn volumes(&self) -> Vec<Volume> {
        self.app.volumes()
    }

    /// The machine's pools as the menu shows them, the imported ones and
    /// those the last scan found, read alone: no kernel is read, and no
    /// device scanned.
    pub(crate) fn pools(&self) -> Vec<Pool> {
        self.app.known_pools()
    }

    /// Whether `name` is a boot environment of the machine, read alone.
    pub(crate) fn is_environment(&self, name: &zbm_types::DatasetName) -> bool {
        self.app.is_environment(name)
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
        if let Some(asker) = self.asker {
            asker.attach(reports.sender());
        }
        spawn_scoped_traced(scope, move || {
            self.serve(&inbox, &sender);
            if let Some(asker) = self.asker {
                asker.detach();
            }
        });
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
            if let Some(asker) = self.asker {
                asker.performing(&request.action, &request.token);
            }
            let events = self.perform(&request.action, &request.token);
            if let Some(asker) = self.asker {
                asker.performed();
            }
            for event in events {
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

/// Whether `events` say a volume was opened.
fn has_opened(events: &[Event]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            Event::UnlockFinished {
                outcome: UnlockOutcome::Opened,
                ..
            }
        )
    })
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
