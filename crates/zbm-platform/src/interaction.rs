//! The person at the console, as a port: what domain code asks of them
//! ([`PromptRequest`], [`SecretRequest`]), waits for ([`WaitRequest`]) and
//! tells them ([`Tell`]), and the [`Interaction`] front ends implement.
//!
//! The requests are data a front end can show its own way: the menu answers
//! them with its dialogs (ROADMAP.md, R5 step 4), the start-up program with
//! the console prompts of the legacy runtime.

use crate::Runner;
use crate::secret::Secret;
pub use zbm_types::Answer;
use zbm_types::RetryDelay;

/// The volume a request is about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeRef<'r> {
    /// The name of its mapping.
    pub name: &'r str,
    /// Its LUKS device.
    pub device: &'r str,
}

/// A secret a volume asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SecretKind {
    /// The volume's passphrase; empty skips the volume.
    Passphrase,
    /// The PIN of its TPM2 token.
    Tpm2Pin,
    /// The PIN of a FIDO2 security key.
    Fido2Pin,
    /// The passphrase YubiKey challenge-response turns into the key, with
    /// the slot.
    YubiKeyPassphrase(String),
    /// The passphrase of a GnuPG key file encrypted with one.
    KeyFilePassphrase(String),
    /// The passphrase of the GnuPG secret key a key file is encrypted to.
    SecretKeyPassphrase(String),
    /// The PIN of an OpenPGP card, with its serial number and the attempts
    /// left, as the card says them.
    CardPin {
        /// The card's serial number.
        serial: String,
        /// The attempts left.
        left: String,
    },
}

impl SecretKind {
    /// Whether an empty answer skips the volume, rather than moving on to
    /// its next key method.
    pub fn is_volume_skipped_on_empty(&self) -> bool {
        *self == SecretKind::Passphrase
    }
}

/// A request for a secret.
pub struct SecretRequest<'r> {
    /// The volume.
    pub volume: VolumeRef<'r>,
    /// What is asked.
    pub kind: SecretKind,
    /// The attempt, from 1: a later one follows a secret that was wrong.
    pub attempt: u32,
    /// The attempts allowed, when limited.
    pub tries: Option<u32>,
    /// Whether another program opened the volume meanwhile, such as `zluks`
    /// over SSH; asked while the request waits, when it can happen.
    pub opened_elsewhere: Option<&'r dyn Fn() -> bool>,
    /// The volume described for the person, as the passphrase prompt names
    /// it: its device, with its label and size when they are known.
    pub description: Option<&'r str>,
}

impl SecretRequest<'_> {
    /// The attempts left, this one included, when limited.
    pub fn tries_left(&self) -> Option<u32> {
        self.tries
            .map(|tries| tries.saturating_sub(self.attempt.saturating_sub(1)))
    }
}

/// What a [`SecretRequest`] got.
#[derive(Debug)]
pub enum Asked {
    /// What was typed: empty to skip the volume or use another method.
    Secret(Secret),
    /// Nothing: another program opened the volume meanwhile.
    Elsewhere,
    /// Nothing: the console has no more input.
    Ended,
}

impl Asked {
    /// The secret typed; `None` when nothing was.
    pub fn secret(self) -> Option<Secret> {
        match self {
            Asked::Secret(secret) => Some(secret),
            Asked::Elsewhere | Asked::Ended => None,
        }
    }
}

impl From<Option<Secret>> for Asked {
    /// What the console read: a secret, or nothing when its input ended.
    fn from(read: Option<Secret>) -> Asked {
        read.map_or(Asked::Ended, Asked::Secret)
    }
}

/// What a volume waits for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Awaited {
    /// A FIDO2 security key.
    SecurityKey,
    /// The OpenPGP security key or card a key file is encrypted to.
    OpenPgpKey,
    /// A YubiKey for challenge-response.
    YubiKey,
    /// The device holding a key file.
    KeyDevice {
        /// The device, as the configuration names it.
        spec: String,
        /// The key file on it.
        file: String,
    },
}

/// A request to wait for a device.
pub struct WaitRequest<'r> {
    /// The volume.
    pub volume: VolumeRef<'r>,
    /// What it waits for.
    pub awaited: Awaited,
    /// How long, in seconds; any key stops waiting.
    pub seconds: u64,
    /// Whether it is there.
    pub present: &'r dyn Fn() -> bool,
}

/// What unlocking says along the way.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Told {
    /// It is working, which may take a while: `Unlocking /dev/sda2 ...`.
    Working,
    /// The user has something to do: touch the security key.
    Touch,
    /// Something did not work: the secret was wrong, the TPM refused.
    Refused,
    /// What comes next is about this: `Unlock /dev/sda2 with a FIDO2
    /// security key`.
    Heading,
}

/// A message of [`Told`] kind, as plain text, a line without its newline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tell<'r> {
    /// What kind of message.
    pub told: Told,
    /// The message.
    pub text: &'r str,
    /// A part of the message to show highlighted, such as the device.
    pub highlight: Option<&'r str>,
}

impl<'r> Tell<'r> {
    /// A message of kind `told`, without a highlighted part.
    pub fn new(told: Told, text: &'r str) -> Tell<'r> {
        Tell {
            told,
            text,
            highlight: None,
        }
    }
}

/// What a timed prompt of the LUKS code is about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Subject {
    /// A round of passphrases did not open a volume: RETURN tries again,
    /// ESCAPE skips it.
    Retry {
        /// The name of its mapping.
        name: String,
        /// Its device.
        device: String,
    },
    /// A device of the configuration is missing: RETURN looks again,
    /// ESCAPE skips it.
    Missing(String),
}

/// A timed prompt of the LUKS code: a message, what RETURN and ESCAPE do,
/// and the countdown "Retrying in NN seconds".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptRequest {
    /// What it is about, for a front end that shows it otherwise.
    pub subject: Subject,
    /// `-d`: the delay; `None` for the default.
    pub delay: Option<RetryDelay>,
    /// `-m`: the message, shown in red.
    pub message: String,
    /// `-r`: what RETURN does.
    pub on_return: String,
    /// `-e`: what ESCAPE does.
    pub on_escape: String,
}

impl PromptRequest {
    /// The arguments of the legacy call, with the colours left out, as the
    /// unit suites record them.
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = Vec::new();
        if let Some(delay) = &self.delay {
            arguments.extend(["-d".to_owned(), delay.to_string()]);
        }
        arguments.extend([
            "-m".to_owned(),
            self.message.clone(),
            "-r".to_owned(),
            self.on_return.clone(),
            "-e".to_owned(),
            self.on_escape.clone(),
            "-p".to_owned(),
            "Retrying in %0.2d seconds".to_owned(),
        ]);
        arguments
    }
}

/// The person at the console, as domain code reaches them: what it asks,
/// says and waits for. Front ends implement it: the plain console
/// (`zbm_tools::console::ConsoleInteraction`), the menu with its dialogs;
/// the unit suites record and answer the requests. Domain code only says
/// what it needs: how each request is shown is the implementation's.
pub trait Interaction {
    /// Shows the prompt and returns what the user did: RETURN or letting the
    /// time pass is [`Answer::Continue`], ESCAPE is [`Answer::Escape`].
    fn ask(&self, request: &PromptRequest) -> Answer;

    /// Asks for a secret.
    fn secret(&self, request: &SecretRequest<'_>) -> Asked;

    /// Waits for a device, while it is not there.
    fn wait(&self, request: &WaitRequest<'_>) -> Wait;

    /// Says something along the way.
    fn notify(&self, tell: Tell<'_>);

    /// Runs `run`, whose program needs the console: an interactive key
    /// script, or `fido2-assert` asking for its PIN. A front end that draws
    /// on the console steps aside meanwhile and gives `run` the runner whose
    /// programs reach it; on the plain console `run` gets `None` and uses
    /// the caller's own runner. `run` must not ask anything.
    fn lease_console(&self, run: &mut dyn FnMut(Option<&dyn Runner>)) {
        run(None);
    }
}

/// No one at the console, for code that must not ask anything, such as a
/// report: timed prompts are escaped, no secret comes, no device is waited
/// for, and nothing is said.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoInteraction;

impl Interaction for NoInteraction {
    fn ask(&self, _: &PromptRequest) -> Answer {
        Answer::Escape
    }

    fn secret(&self, _: &SecretRequest<'_>) -> Asked {
        Asked::Ended
    }

    fn wait(&self, request: &WaitRequest<'_>) -> Wait {
        if (request.present)() {
            Wait::Present
        } else {
            Wait::TimedOut
        }
    }

    fn notify(&self, _: Tell<'_>) {}
}

/// How waiting for a device ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum Wait {
    /// What was waited for is there.
    Present,
    /// The user pressed a key to stop waiting.
    Skipped,
    /// The time ran out, or there was no time to wait.
    TimedOut,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tries_left_count_this_attempt_and_only_a_passphrase_skips() {
        let request = |attempt, tries| SecretRequest {
            volume: VolumeRef {
                name: "root",
                device: "/dev/sda2",
            },
            kind: SecretKind::Tpm2Pin,
            attempt,
            tries,
            opened_elsewhere: None,
            description: None,
        };
        assert_eq!(request(1, Some(3)).tries_left(), Some(3));
        assert_eq!(request(3, Some(3)).tries_left(), Some(1));
        assert_eq!(request(5, Some(3)).tries_left(), Some(0));
        assert_eq!(request(2, None).tries_left(), None);
        assert!(SecretKind::Passphrase.is_volume_skipped_on_empty());
        assert!(!SecretKind::Fido2Pin.is_volume_skipped_on_empty());
        assert!(Asked::Secret(Secret::copy_from(b"pw")).secret().is_some());
        assert!(Asked::Elsewhere.secret().is_none() && Asked::Ended.secret().is_none());
        assert!(matches!(Asked::from(None), Asked::Ended));
    }

    #[test]
    fn no_one_answers_nothing_and_lends_no_console() {
        let volume = VolumeRef {
            name: "root",
            device: "/dev/sda2",
        };
        let request = PromptRequest {
            subject: Subject::Missing("LABEL=keys".to_owned()),
            delay: None,
            message: "missing".to_owned(),
            on_return: "retry".to_owned(),
            on_escape: "skip".to_owned(),
        };
        assert_eq!(NoInteraction.ask(&request), Answer::Escape);
        let secret = SecretRequest {
            volume,
            kind: SecretKind::Fido2Pin,
            attempt: 1,
            tries: None,
            opened_elsewhere: None,
            description: None,
        };
        assert!(matches!(NoInteraction.secret(&secret), Asked::Ended));
        let wait = |present: &dyn Fn() -> bool| {
            NoInteraction.wait(&WaitRequest {
                volume,
                awaited: Awaited::YubiKey,
                seconds: 5,
                present,
            })
        };
        assert_eq!(wait(&|| true), Wait::Present);
        assert_eq!(wait(&|| false), Wait::TimedOut);
        NoInteraction.notify(Tell::new(Told::Working, "Unlocking"));
        let mut lent = None;
        NoInteraction.lease_console(&mut |runner| lent = Some(runner.is_some()));
        assert_eq!(lent, Some(false));
    }
}
