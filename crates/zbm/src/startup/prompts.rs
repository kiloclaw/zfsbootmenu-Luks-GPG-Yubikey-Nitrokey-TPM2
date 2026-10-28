//! What unlocking asks, answered with the start-up view's dialogs: a
//! secret in the unlock dialog, a device waited for in it with the time
//! left, and the timed prompts as questions.

use std::time::Duration;

use zbm_app::model::{Action, Event, Method, Notice, Question, Volume, VolumeState};
use zbm_boot::prompt::retry_delay;
use zbm_platform::Clock;
use zbm_platform::interaction::{
    Asked, Awaited, SecretKind, SecretRequest, Tell, Told, VolumeRef, WaitRequest,
};
use zbm_platform::interaction::{Interaction, PromptRequest, Subject, Wait};
use zbm_platform::secret::Secret;
use zbm_types::Answer;

use super::{DEFAULT_DELAY, MenuFront};

/// What the console says when a passphrase did not open the volume, which
/// the next prompt says too.
const WRONG_PASSPHRASE: &str = "No key available with this passphrase.";

/// The method of the unlock dialog that asks for `kind`.
fn secret_method(kind: &SecretKind) -> Method {
    match kind {
        SecretKind::Passphrase => Method::Passphrase,
        SecretKind::Tpm2Pin => Method::Tpm2Pin,
        SecretKind::Fido2Pin => Method::Fido2Pin,
        SecretKind::YubiKeyPassphrase(_) => Method::YubiKeyPassphrase,
        SecretKind::KeyFilePassphrase(_) | SecretKind::SecretKeyPassphrase(_) => {
            Method::GnupgKeyFile
        }
        SecretKind::CardPin { .. } => Method::OpenPgpCardPin,
    }
}

/// The method of the unlock dialog that waits for `awaited`.
fn awaited_method(awaited: &Awaited) -> Method {
    match awaited {
        Awaited::SecurityKey => Method::Fido2,
        Awaited::OpenPgpKey => Method::OpenPgpCard,
        Awaited::YubiKey => Method::YubiKey,
        Awaited::KeyDevice { .. } => Method::KeyDevice,
    }
}

/// Says when a second passed on `clock`, the first time at once.
struct EverySecond<'c> {
    clock: &'c dyn Clock,
    last: Option<Duration>,
}

impl<'c> EverySecond<'c> {
    fn new(clock: &'c dyn Clock) -> EverySecond<'c> {
        EverySecond { clock, last: None }
    }

    /// Whether a second passed since this last said so.
    fn is_due(&mut self) -> bool {
        let now = self.clock.now();
        let due = self
            .last
            .is_none_or(|last| now.saturating_sub(last) >= Duration::from_secs(1));
        if due {
            self.last = Some(now);
        }
        due
    }
}

impl MenuFront<'_> {
    /// Why the dialog asks for `method`, attempt `attempt`: what unlocking
    /// said went wrong since the last request, a wrong secret when the same
    /// method asks again, or another method that failed.
    fn notice(&self, method: Method, attempt: u32) -> Option<Notice> {
        let refused = self.refused.take();
        let again = self.last.replace(Some(method)) == Some(method) && attempt > 1;
        if !refused.is_empty() {
            let text = refused.join(" ");
            return Some(if again {
                Notice::Wrong(text)
            } else {
                Notice::Instead(text)
            });
        }
        (method == Method::Passphrase && attempt > 1)
            .then(|| Notice::Wrong(WRONG_PASSPHRASE.to_owned()))
    }

    /// Lists `volume`, asked for with `method`, among the view's volumes,
    /// so that its dialog names its device.
    fn list_volume(&self, volume: VolumeRef<'_>, method: Method) {
        let _ = self.with_view(|view| {
            let mut snapshot = view.snapshot().clone();
            let known = Volume {
                name: volume.name.to_owned(),
                device: volume.device.to_owned(),
                state: VolumeState::Locked,
                methods: vec![method],
                pool: None,
                tpm2: None,
            };
            match snapshot.volumes.iter_mut().find(|v| v.name == volume.name) {
                Some(listed) => *listed = known,
                None => snapshot.volumes.push(known),
            }
            view.send(Event::Changed(snapshot))
        });
    }
}

impl Interaction for MenuFront<'_> {
    fn ask(&self, request: &PromptRequest) -> Answer {
        let question = match &request.subject {
            Subject::Retry { name, .. } => Question::RetryUnlock {
                volume: name.clone(),
            },
            Subject::Missing(device) => Question::MissingVolume {
                device: device.clone(),
            },
        };
        let delay = request.delay.map_or(DEFAULT_DELAY, retry_delay);
        self.timed(question, delay)
    }

    /// The unlock dialog: what was typed with Enter; empty with Esc, or when
    /// another method was chosen; nothing when another program opened the
    /// volume meanwhile, or the view is gone.
    fn secret(&self, request: &SecretRequest<'_>) -> Asked {
        let method = secret_method(&request.kind);
        let notice = self.notice(method, request.attempt);
        self.list_volume(request.volume, method);
        let event = Event::UnlockRequested {
            volume: request.volume.name.to_owned(),
            method,
            tries_left: request.tries_left(),
            notice,
            wait: None,
        };
        let mut every = EverySecond::new(self.system.clock);
        let mut elsewhere = || {
            let opened = request.opened_elsewhere?;
            (every.is_due() && opened()).then_some(Action::Answer(Answer::Escape))
        };
        match self.with_view(|view| view.ask(&event, &mut elsewhere)) {
            Some(Action::Unlock {
                method: chosen,
                secret,
                ..
            }) if chosen == method => Asked::Secret(
                secret
                    .map(zbm_platform::secret::unshare)
                    .unwrap_or_default(),
            ),
            Some(Action::Answer(_)) => Asked::Elsewhere,
            Some(_) => Asked::Secret(Secret::new()),
            None => Asked::Ended,
        }
    }

    /// The unlock dialog, waiting for the device with the time left: Esc
    /// stops waiting, Enter looks again at once.
    fn wait(&self, request: &WaitRequest<'_>) -> Wait {
        if (request.present)() {
            return Wait::Present;
        }
        let method = awaited_method(&request.awaited);
        let notice = self.notice(method, 1);
        self.list_volume(request.volume, method);
        let clock = self.system.clock;
        let start = clock.now();
        let mut every = EverySecond::new(clock);
        let _ = every.is_due();
        loop {
            let spent = clock.now().saturating_sub(start).as_secs();
            let left = request.seconds.saturating_sub(spent);
            if left == 0 {
                return Wait::TimedOut;
            }
            let event = Event::UnlockRequested {
                volume: request.volume.name.to_owned(),
                method,
                tries_left: None,
                notice: notice.clone(),
                wait: Some(left),
            };
            let mut present = || {
                (every.is_due() && (request.present)()).then_some(Action::Answer(Answer::Continue))
            };
            match self.with_view(|view| view.ask(&event, &mut present)) {
                Some(Action::Answer(_)) => return Wait::Present,
                Some(Action::SkipUnlock(_)) => return Wait::Skipped,
                Some(_) if (request.present)() => return Wait::Present,
                Some(_) => {}
                None => return Wait::TimedOut,
            }
        }
    }

    /// The view steps aside while `run`'s program has the console, with the
    /// runner of the whole system rather than the quiet one of the stages.
    fn lease_console(&self, run: &mut dyn FnMut(Option<&dyn zbm_platform::Runner>)) {
        let runner = self.system.runner;
        self.hand_over(|| run(Some(runner)));
    }

    /// What goes wrong is said by the next dialog; what takes a while
    /// shows until the next event.
    fn notify(&self, tell: Tell<'_>) {
        match tell.told {
            Told::Refused => self.refused.borrow_mut().push(tell.text.to_owned()),
            Told::Working | Told::Touch | Told::Heading => {
                self.show(Event::Progress(tell.text.to_owned()));
            }
        }
    }
}
