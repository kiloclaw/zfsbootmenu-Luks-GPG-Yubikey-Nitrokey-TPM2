//! What domain code asks the person at the console, and what it gets
//! back, as data that crosses threads and processes (ROADMAP.md, PM8):
//! the questions of `zbm_platform::interaction`'s `Interaction` port that
//! borrow nothing.

use crate::RetryDelay;

/// A secret a volume or a ZFS encryption root asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SecretKind {
    /// The volume's passphrase, where empty skips the volume; or the
    /// passphrase of an encryption root (`keyformat=passphrase`).
    Passphrase,
    /// The hexadecimal key of an encryption root (`keyformat=hex`). A raw
    /// key is never asked for: `zfs load-key` reads one from a file only.
    HexKey,
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

/// What unlocking and booting say along the way.
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
    /// A kernel boots: `Booting /boot/vmlinuz-6.1 for zroot/ROOT/void
    /// ...`, with the kernel highlighted.
    Booting,
}

/// What a timed prompt is about: a volume of the LUKS code, or a boot that
/// failed.
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
    /// The kernel and initramfs could not be loaded into memory; the menu
    /// comes back when the prompt ends.
    LoadFailed {
        /// The kernel's path, below the mounted boot environment.
        kernel: String,
        /// The initramfs's path, below the mounted boot environment.
        initramfs: String,
    },
    /// `kexec` returned: the kernel did not boot; the menu comes back when
    /// the prompt ends.
    KexecFailed {
        /// The kernel's path in its boot environment.
        kernel: String,
    },
}

impl Subject {
    /// Whether the prompt is about a boot that failed: it has no RETURN
    /// and ESCAPE lines, and counts down as "Press RETURN to continue".
    pub fn is_failed_boot(&self) -> bool {
        matches!(
            self,
            Subject::LoadFailed { .. } | Subject::KexecFailed { .. }
        )
    }
}

/// A timed prompt: a message, what RETURN and ESCAPE do, and the
/// countdown, "Retrying in NN seconds" for the LUKS code's. A failed boot's
/// prompt ([`Subject::is_failed_boot`]) has neither RETURN nor ESCAPE text:
/// both are empty.
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
        match &self.subject {
            Subject::LoadFailed { kernel, initramfs } => {
                arguments.extend(["-m".to_owned(), self.message.clone()]);
                arguments.extend(["-m".to_owned(), kernel.clone()]);
                arguments.extend(["-m".to_owned(), initramfs.clone()]);
                return arguments;
            }
            Subject::KexecFailed { .. } => {
                arguments.extend(["-m".to_owned(), self.message.clone()]);
                return arguments;
            }
            Subject::Retry { .. } | Subject::Missing(_) => {}
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

/// The answer to the resume question: a suspend partition may hold a
/// hibernated system, and importing a pool read-write before that system
/// resumes can destroy the pool (ADR 0015).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum Resume {
    /// Go on, and give the boot environment `noresume`.
    Noresume,
    /// Go on without `noresume`: the user adds it where needed.
    Dangerous,
    /// Keep the pool read-only.
    Refuse,
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

    fn request(subject: Subject, message: &str) -> PromptRequest {
        PromptRequest {
            subject,
            delay: RetryDelay::new(10),
            message: message.to_owned(),
            on_return: String::new(),
            on_escape: String::new(),
        }
    }

    #[test]
    fn a_failed_boot_prompts_with_its_messages_alone() {
        let load = request(
            Subject::LoadFailed {
                kernel: "/mnt/boot/vmlinuz".to_owned(),
                initramfs: "/mnt/boot/initramfs.img".to_owned(),
            },
            "Unable to load kernel or initramfs into memory",
        );
        assert!(load.subject.is_failed_boot());
        assert_eq!(
            load.arguments(),
            [
                "-d",
                "10",
                "-m",
                "Unable to load kernel or initramfs into memory",
                "-m",
                "/mnt/boot/vmlinuz",
                "-m",
                "/mnt/boot/initramfs.img"
            ]
        );
        let kexec = request(
            Subject::KexecFailed {
                kernel: "/boot/vmlinuz".to_owned(),
            },
            "kexec run of /boot/vmlinuz failed!",
        );
        assert_eq!(
            kexec.arguments(),
            ["-d", "10", "-m", "kexec run of /boot/vmlinuz failed!"]
        );
        let retry = request(Subject::Missing("LABEL=keys".to_owned()), "missing");
        assert!(!retry.subject.is_failed_boot());
        assert_eq!(
            retry.arguments().last().map(String::as_str),
            Some("Retrying in %0.2d seconds")
        );
    }
}
