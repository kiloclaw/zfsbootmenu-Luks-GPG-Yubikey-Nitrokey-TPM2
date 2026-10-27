//! What unlocking asks of the person at the console, as requests a front
//! end can show: a secret to type ([`SecretRequest`]), a device to plug in
//! ([`WaitRequest`]), and what it says along the way ([`Tell`]).
//!
//! The [`Prompter`](crate::unlocker::Prompter) answers them. Its default
//! answers are the plain console prompts, which each call site gives as a
//! closure, so that the console shows what it showed before the requests
//! existed; the menu answers them with its dialogs (ROADMAP.md, R5
//! step 4).

use zbm_platform::secret::Secret;

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
