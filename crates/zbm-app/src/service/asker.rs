//! What unlocking asks in the menu (ADR 0016, point 1): while the
//! machine's service opens a volume, the keyholder's questions, a secret,
//! a device to wait for, a volume to retry, reach the menu's dialogs
//! through the service's mailbox ([`Report::Ask`]), and the answers come
//! straight back on each question's reply, never through the service's
//! queue, which the action that asks holds. The dialogs are start-up's
//! ([`Dialogs`], [`Asks`]).
//!
//! A question ends unanswered when the action is cancelled, as when the
//! user cancels it or another instance takes the menu over, or when
//! nothing reads the mailbox any more: the dialog is withdrawn
//! ([`Report::Withdraw`]) and unlocking is told nothing came. A program
//! that needs the console is lent none: the menu draws on it.

mod dialogs;

use std::sync::Mutex;
use std::time::Duration;

use zbm_platform::Clock;
use zbm_platform::event::{CancelToken, MailSender, NoAnswer, reply};
use zbm_platform::interaction::{
    Asked, Interaction, PromptRequest, SecretRequest, SecretSubject, Tell, VolumeRef, Wait,
    WaitRequest,
};
use zbm_types::Answer;

use self::dialogs::lock;
pub use self::dialogs::{Asks, Dialogs};
use super::Report;
use crate::model::{Action, Event, Method};

/// How often a question that waits looks at what else may end it: the
/// action cancelled, the device it waits for, another program.
const POLL: Duration = Duration::from_millis(50);

/// The menu's side of what unlocking asks, which the LUKS session of the
/// menu's application asks ([`zbm_luks::unlocker::Luks::interaction`]),
/// and the service it runs in attaches to its mailbox
/// ([`MachineService::with_asker`](super::MachineService::with_asker)).
/// Asked outside an action of that service, nothing answers.
pub struct Asker<'a> {
    clock: &'a dyn Clock,
    dialogs: Dialogs,
    serving: Mutex<Serving>,
}

/// What the asker asks through.
#[derive(Default)]
struct Serving {
    /// The service's mailbox, while the service runs.
    reports: Option<MailSender<Report>>,
    /// The action performed now: what cancels it, and the name the menu
    /// gives the volume it opens.
    performing: Option<(CancelToken, Option<String>)>,
}

impl<'a> Asker<'a> {
    /// The asker, whose dialogs count on `clock`.
    pub fn new(clock: &'a dyn Clock) -> Asker<'a> {
        Asker {
            clock,
            dialogs: Dialogs::default(),
            serving: Mutex::new(Serving::default()),
        }
    }

    /// The service started: questions go to `reports`.
    pub(crate) fn attach(&self, reports: MailSender<Report>) {
        lock(&self.serving).reports = Some(reports);
    }

    /// The service ended: nothing answers any more.
    pub(crate) fn detach(&self) {
        *lock(&self.serving) = Serving::default();
    }

    /// The service performs `action`, which `token` cancels.
    pub(crate) fn performing(&self, action: &Action, token: &CancelToken) {
        // The menu's snapshot names a volume by its recorded mapping, which
        // it opens it as, or else by its device (`read::volumes`).
        let name = match action {
            Action::Open { device, name } => Some(
                name.as_ref()
                    .map_or_else(|| device.clone(), ToString::to_string),
            ),
            _ => None,
        };
        lock(&self.serving).performing = Some((token.clone(), name));
    }

    /// The action ended.
    pub(crate) fn performed(&self) {
        lock(&self.serving).performing = None;
    }
}

impl Asks for Asker<'_> {
    fn show(&self, event: Event) {
        let reports = lock(&self.serving).reports.clone();
        if let Some(reports) = reports {
            let _ = reports.send(Report::Event(Box::new(event)));
        }
    }

    /// The answer the menu sends to the question's reply, or `check`'s;
    /// `None` once the action is cancelled or nothing reads the mailbox.
    fn answer(
        &self,
        event: Event,
        mut check: Option<&mut dyn FnMut() -> Option<Action>>,
    ) -> Option<Action> {
        let (reports, token) = {
            let serving = lock(&self.serving);
            (
                serving.reports.clone()?,
                serving.performing.as_ref()?.0.clone(),
            )
        };
        // A cancelled action asks nothing more.
        if token.is_cancelled() {
            return None;
        }
        let (answer, awaiting) = reply();
        let asked = Report::Ask {
            event: Box::new(event),
            reply: answer,
        };
        reports.send(asked).ok()?;
        loop {
            match awaiting.wait(Some(POLL)) {
                // An answer that came once the action was cancelled is none.
                Ok(action) => return (!token.is_cancelled()).then_some(action),
                Err(NoAnswer::Gone) => return None,
                Err(NoAnswer::NotYet) => {}
            }
            if token.is_cancelled() {
                let _ = reports.send(Report::Withdraw);
                return None;
            }
            if let Some(checked) = check.as_mut().and_then(|check| check()) {
                let _ = reports.send(Report::Withdraw);
                return Some(checked);
            }
        }
    }

    fn list(&self, volume: VolumeRef<'_>, _method: Method) -> String {
        let serving = lock(&self.serving);
        serving
            .performing
            .as_ref()
            .and_then(|(_, name)| name.clone())
            .unwrap_or_else(|| volume.name.to_owned())
    }

    fn clock(&self) -> &dyn Clock {
        self.clock
    }
}

impl Interaction for Asker<'_> {
    /// A volume to retry or a device that is missing, as a timed question;
    /// a boot's prompts are not unlocking's, and stop.
    fn ask(&self, request: &PromptRequest) -> Answer {
        Dialogs::ask(self, request).unwrap_or(Answer::Escape)
    }

    /// The unlock dialog; a ZFS key is not unlocking's, and none comes.
    fn secret(&self, request: &SecretRequest<'_>) -> Asked {
        match request.subject {
            SecretSubject::Volume(volume) => self.dialogs.secret(self, volume, request),
            SecretSubject::EncryptionRoot(_) => Asked::Ended,
        }
    }

    fn wait(&self, request: &WaitRequest<'_>) -> Wait {
        self.dialogs.wait(self, request)
    }

    fn notify(&self, tell: Tell<'_>) {
        self.dialogs.notify(self, tell);
    }
}

#[cfg(test)]
mod tests;
