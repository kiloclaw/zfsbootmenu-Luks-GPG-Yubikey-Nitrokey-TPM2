//! Start-up's stages and the start-up view, on two threads (ADR 0001,
//! PM8): the stages run on a worker and reach the view through a
//! [`ViewLink`]; the view runs on the thread that owns the terminal, and
//! [`StartupView::serve`](super::StartupView::serve) takes what they send
//! from its [`Mailbox`](zbm_platform::event::Mailbox). A question is a
//! request with a one-shot [`Reply`]: the worker waits for its answer, and
//! looks meanwhile at what it waits for, a device or another program, which
//! may answer the question first.

use zbm_platform::event::{Awaiting, MailSender, Reply, reply};

use crate::menu::world::{Action, Event};

/// What the stages send the view.
#[derive(Debug)]
pub enum ToView {
    /// Shows an event: a stage started or ended, a volume opened.
    Show(Box<Event>),
    /// Opens the dialog the event opens; its answer goes to `reply`.
    Ask {
        /// What is asked.
        event: Box<Event>,
        /// Where the answer goes.
        reply: Reply<Action>,
    },
    /// The question asked last was answered otherwise, as by the device it
    /// waited for: its dialog closes.
    Withdraw,
    /// Gives the terminal to the stages' programs, hooks or the recovery
    /// shell, saying so on `ready`, until [`ToView::Resume`].
    Lend {
        /// Told once the terminal is given.
        ready: Reply<()>,
    },
    /// Takes the terminal back.
    Resume,
    /// Start-up is over: the view gives the terminal back, says so on
    /// `done`, and ends.
    Close {
        /// Told once the terminal is given back.
        done: Reply<()>,
    },
}

/// The start-up view as the stages reach it from their thread. Once the
/// view is gone, as when its terminal failed, nothing is shown and nothing
/// answers.
pub trait ViewLink: Send + Sync {
    /// Shows `event`; `false` once the view is gone.
    fn show(&self, event: Event) -> bool;

    /// Opens the dialog `event` opens; its answer comes to what this
    /// returns, `None` once the view is gone.
    fn ask(&self, event: Event) -> Option<Awaiting<Action>>;

    /// Closes the dialog of the last question, which was answered
    /// otherwise.
    fn withdraw(&self);

    /// Gives the terminal to the stages' programs: returns once the view
    /// gave it up, or is gone.
    fn lend(&self);

    /// Takes the terminal back after [`ViewLink::lend`].
    fn resume(&self);

    /// Ends the view: returns once it gave the terminal back, or is gone.
    fn close(&self);
}

/// The link through the view's mailbox.
#[derive(Clone)]
pub struct MailLink(MailSender<ToView>);

impl MailLink {
    /// The link that sends to `sender`, the view's mailbox.
    pub fn new(sender: MailSender<ToView>) -> MailLink {
        MailLink(sender)
    }

    /// Sends `message`, waits for `answer` unless it could not, and says
    /// whether the view took it.
    fn is_taken(&self, message: ToView, answer: &Awaiting<()>) -> bool {
        self.0.send(message).is_ok() && answer.wait(None).is_ok()
    }
}

impl ViewLink for MailLink {
    fn show(&self, event: Event) -> bool {
        self.0.send(ToView::Show(Box::new(event))).is_ok()
    }

    fn ask(&self, event: Event) -> Option<Awaiting<Action>> {
        let (answer, awaiting) = reply();
        let asked = ToView::Ask {
            event: Box::new(event),
            reply: answer,
        };
        self.0.send(asked).ok()?;
        Some(awaiting)
    }

    fn withdraw(&self) {
        let _ = self.0.send(ToView::Withdraw);
    }

    fn lend(&self) {
        let (ready, awaiting) = reply();
        let _ = self.is_taken(ToView::Lend { ready }, &awaiting);
    }

    fn resume(&self) {
        let _ = self.0.send(ToView::Resume);
    }

    fn close(&self) {
        let (done, awaiting) = reply();
        let _ = self.is_taken(ToView::Close { done }, &awaiting);
    }
}
