//! The person at the console, as a port: what domain code asks of them
//! ([`PromptRequest`], [`SecretRequest`]), waits for ([`WaitRequest`]) and
//! tells them ([`Tell`]), and the [`Interaction`] front ends implement.
//!
//! The requests are data a front end can show its own way: the menu answers
//! them with its dialogs (ROADMAP.md, R5 step 4), the start-up program with
//! the console prompts of the legacy runtime.

use crate::System;
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
}

/// A message of [`Told`] kind, as plain text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tell<'r> {
    /// What kind of message.
    pub told: Told,
    /// The message.
    pub text: &'r str,
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
/// says and waits for. Front ends implement it: the start-up program on the
/// console, the menu with its dialogs; the unit suites record and answer the
/// timed prompts. Every request but the timed prompt comes with the plain
/// console prompt as a closure, which the defaults run (ROADMAP.md, DM2:
/// the closures go when the plain console becomes an implementation too).
pub trait Interaction {
    /// Shows the prompt and returns what the user did: RETURN or letting the
    /// time pass is [`Answer::Continue`], ESCAPE is [`Answer::Escape`].
    fn ask(&self, request: &PromptRequest) -> Answer;

    /// Asks for a secret; `console` asks on the plain console.
    fn secret(&self, _request: &SecretRequest<'_>, console: &mut dyn FnMut() -> Asked) -> Asked {
        console()
    }

    /// Waits for a device; `console` waits on the plain console.
    fn wait(&self, _request: &WaitRequest<'_>, console: &mut dyn FnMut() -> Wait) -> Wait {
        console()
    }

    /// Says something along the way; `console` says it on the plain console.
    fn notify(&self, _tell: Tell<'_>, console: &mut dyn FnMut()) {
        console();
    }

    /// Runs `run`, whose program needs the console: an interactive key
    /// script, or `fido2-assert` asking for its PIN. A front end that draws
    /// on the console steps aside meanwhile and gives `run` the system whose
    /// programs reach it; on the plain console `run` gets `None` and uses
    /// the unlocking's own system. `run` must not ask the prompter anything.
    fn lease_console(&self, run: &mut dyn FnMut(Option<&System<'_>>)) {
        run(None);
    }
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
}
