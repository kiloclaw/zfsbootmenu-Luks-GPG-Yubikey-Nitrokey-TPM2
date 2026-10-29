//! What domain code asks the person at the console, and what it gets
//! back, as data that crosses threads and processes (ROADMAP.md, PM8):
//! the questions of `zbm_platform::interaction`'s `Interaction` port that
//! borrow nothing.

use crate::RetryDelay;

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
