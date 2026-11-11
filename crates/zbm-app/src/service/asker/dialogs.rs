//! What unlocking asks, answered with the menu's dialogs: a secret in the
//! unlock dialog, a device waited for in it with the time left, and the
//! timed questions. Start-up's front end and the menu's
//! [`Asker`](super::Asker) share it; each says how its dialogs open and
//! are answered ([`Asks`]).

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use zbm_platform::Clock;
use zbm_platform::interaction::{
    Asked, Awaited, PromptRequest, SecretKind, SecretRequest, Subject, Tell, Told, VolumeRef, Wait,
    WaitRequest,
};
use zbm_platform::secret::Secret;
use zbm_types::Answer;

use crate::model::{Action, Event, Method, Notice, Question};

/// What the console says when a passphrase did not open the volume, which
/// the next prompt says too.
const WRONG_PASSPHRASE: &str = "No key available with this passphrase.";

/// A front end whose dialogs answer what unlocking asks.
pub trait Asks: Sync {
    /// Shows `event`, such as what takes a while.
    fn show(&self, event: Event);

    /// The answer to `event`, from its dialog, or from `check`, which looks
    /// at what else may answer it while the dialog waits, and then closes
    /// the dialog; `None` when nothing will answer: the front end is gone,
    /// or the action that asks was cancelled.
    fn answer(
        &self,
        event: Event,
        check: Option<&mut dyn FnMut() -> Option<Action>>,
    ) -> Option<Action>;

    /// The name the dialog gives `volume`, asked for with `method`, once
    /// the front end lists it among its volumes, so that the dialog names
    /// its device and its other methods.
    fn list(&self, volume: VolumeRef<'_>, method: Method) -> String;

    /// The clock the dialogs count on.
    fn clock(&self) -> &dyn Clock;
}

/// The method of the unlock dialog that asks for `kind`.
fn secret_method(kind: &SecretKind) -> Method {
    match kind {
        // A volume asks for no ZFS key.
        SecretKind::Passphrase | SecretKind::HexKey => Method::Passphrase,
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

/// The question of the timed prompt about `subject`, when it is one of
/// unlocking's: a volume to retry, a device that is missing. A failed
/// boot's and a chroot's are asked on the console, which the menu yielded.
fn question(subject: &Subject) -> Option<Question> {
    match subject {
        Subject::Retry { name, .. } => Some(Question::RetryUnlock {
            volume: name.clone(),
        }),
        Subject::Missing(device) => Some(Question::MissingVolume {
            device: device.clone(),
        }),
        Subject::LoadFailed { .. } | Subject::KexecFailed { .. } | Subject::Chroot { .. } => None,
    }
}

/// What the unlock dialogs keep from one request to the next: why the
/// next one asks.
#[derive(Debug, Default)]
pub struct Dialogs {
    /// What unlocking said went wrong since the last request.
    refused: Mutex<Vec<String>>,
    /// The method the last request asked for.
    last: Mutex<Option<Method>>,
}

/// `mutex`'s value, also after a panic elsewhere.
pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Dialogs {
    /// How long a question waits when nothing says, as the console prompts
    /// wait: 30 seconds.
    pub const DEFAULT_DELAY: i64 = 30;

    /// Asks `question` with `asks`, going on by itself after `delay`
    /// seconds (0: at once, below 0: never), as the console's timed prompts
    /// do; it stops when nothing answers.
    pub fn timed(asks: &dyn Asks, question: Question, delay: i64) -> Answer {
        if delay == 0 {
            return Answer::Continue;
        }
        let seconds = u64::try_from(delay).ok();
        match asks.answer(Event::Asked { question, seconds }, None) {
            Some(Action::Answer(answer)) => answer,
            Some(_) => Answer::Continue,
            None => Answer::Escape,
        }
    }

    /// The timed prompt `request` as a question, through `asks`, when it is
    /// one of unlocking's (a volume to retry, a device that is missing),
    /// after its delay or [`Dialogs::DEFAULT_DELAY`]; `None` for a boot's.
    pub fn ask(asks: &dyn Asks, request: &PromptRequest) -> Option<Answer> {
        let question = question(&request.subject)?;
        let delay = request
            .delay
            .map_or(Dialogs::DEFAULT_DELAY, crate::prompt::retry_delay);
        Some(Dialogs::timed(asks, question, delay))
    }

    /// Why the dialog asks for `method`, attempt `attempt`: what unlocking
    /// said went wrong since the last request, a wrong secret when the same
    /// method asks again, or another method that failed.
    fn notice(&self, method: Method, attempt: u32) -> Option<Notice> {
        let refused = std::mem::take(&mut *lock(&self.refused));
        let again = lock(&self.last).replace(method) == Some(method) && attempt > 1;
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

    /// The unlock dialog of `volume`, through `asks`: what was typed with
    /// Enter; empty with Esc, or when another method was chosen; nothing
    /// when another program opened the volume meanwhile, or nothing
    /// answers.
    pub fn secret(
        &self,
        asks: &dyn Asks,
        volume: VolumeRef<'_>,
        request: &SecretRequest<'_>,
    ) -> Asked {
        let method = secret_method(&request.kind);
        let notice = self.notice(method, request.attempt);
        let event = Event::UnlockRequested {
            volume: asks.list(volume, method),
            method,
            tries_left: request.tries_left(),
            notice,
            wait: None,
        };
        let mut every = EverySecond::new(asks.clock());
        let mut elsewhere = |opened: &dyn Fn() -> bool| {
            (every.is_due() && opened()).then_some(Action::Answer(Answer::Escape))
        };
        let answer = match request.opened_elsewhere {
            Some(opened) => asks.answer(event, Some(&mut || elsewhere(opened))),
            None => asks.answer(event, None),
        };
        match answer {
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

    /// The unlock dialog, through `asks`, waiting for the device with the
    /// time left: Esc stops waiting, Enter looks again at once.
    pub fn wait(&self, asks: &dyn Asks, request: &WaitRequest<'_>) -> Wait {
        if (request.present)() {
            return Wait::Present;
        }
        let method = awaited_method(&request.awaited);
        let notice = self.notice(method, 1);
        let volume = asks.list(request.volume, method);
        let clock = asks.clock();
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
                volume: volume.clone(),
                method,
                tries_left: None,
                notice: notice.clone(),
                wait: Some(left),
            };
            let mut present = || {
                (every.is_due() && (request.present)()).then_some(Action::Answer(Answer::Continue))
            };
            match asks.answer(event, Some(&mut present)) {
                Some(Action::Answer(_)) => return Wait::Present,
                Some(Action::SkipUnlock(_)) => return Wait::Skipped,
                Some(_) if (request.present)() => return Wait::Present,
                Some(_) => {}
                None => return Wait::TimedOut,
            }
        }
    }

    /// What unlocking says, through `asks`: what goes wrong is said by the
    /// next dialog; what takes a while shows until the next event.
    pub fn notify(&self, asks: &dyn Asks, tell: Tell<'_>) {
        match tell.told {
            Told::Refused => lock(&self.refused).push(tell.text.to_owned()),
            Told::Working | Told::Touch | Told::Heading | Told::Booting => {
                asks.show(Event::Progress(tell.text.to_owned()));
            }
        }
    }
}
