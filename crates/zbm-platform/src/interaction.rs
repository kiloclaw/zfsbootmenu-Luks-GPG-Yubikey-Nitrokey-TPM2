//! The person at the console, as a port: what domain code asks of them
//! ([`PromptRequest`], [`SecretRequest`], [`ResumeRequest`]), waits for
//! ([`WaitRequest`]) and tells them ([`Tell`]), and the [`Interaction`]
//! front ends implement.
//!
//! The requests are data a front end can show its own way: the menu answers
//! them with its dialogs (ROADMAP.md, R5 step 4), the start-up program with
//! the console prompts of the legacy runtime.

use crate::Runner;
use crate::secret::Secret;
pub use zbm_types::Answer;
pub use zbm_types::interaction::{Awaited, PromptRequest, Resume, SecretKind, Subject, Told, Wait};

/// The volume a request is about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeRef<'r> {
    /// The name of its mapping.
    pub name: &'r str,
    /// Its LUKS device.
    pub device: &'r str,
}

/// What a secret is asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretSubject<'r> {
    /// A LUKS volume.
    Volume(VolumeRef<'r>),
    /// A ZFS encryption root, whose key `zfs load-key` reads.
    EncryptionRoot(&'r str),
}

impl<'r> SecretSubject<'r> {
    /// The LUKS volume asked about, if it is one.
    pub fn volume(&self) -> Option<VolumeRef<'r>> {
        match self {
            SecretSubject::Volume(volume) => Some(*volume),
            SecretSubject::EncryptionRoot(_) => None,
        }
    }
}

impl<'r> From<VolumeRef<'r>> for SecretSubject<'r> {
    fn from(volume: VolumeRef<'r>) -> SecretSubject<'r> {
        SecretSubject::Volume(volume)
    }
}

/// A request for a secret.
pub struct SecretRequest<'r> {
    /// What it is for: a LUKS volume or a ZFS encryption root.
    pub subject: SecretSubject<'r>,
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

/// The text of [`Told::Booting`]: `Booting KERNEL for ENVIRONMENT ...`.
pub fn booting_text(kernel: &str, environment: &str) -> String {
    format!("Booting {kernel} for {environment} ...")
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

/// The resume question, before `pool` is imported read-write while a
/// suspend partition may hold a hibernated system.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResumeRequest<'r> {
    /// The pool.
    pub pool: &'r str,
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

/// The person at the console, as domain code reaches them: what it asks,
/// says and waits for. Front ends implement it: the plain console
/// (`zbm_tools::console::ConsoleInteraction`), the menu with its dialogs;
/// the unit suites record and answer the requests. Domain code only says
/// what it needs: how each request is shown is the implementation's.
///
/// Unlike the other ports it need not be `Send` or `Sync`: the menu answers
/// on the thread that draws it. ROADMAP.md's PM8 makes each question a
/// request with a one-shot reply, which a worker can send.
pub trait Interaction: Send + Sync {
    /// Shows the prompt and returns what the user did: RETURN or letting the
    /// time pass is [`Answer::Continue`], ESCAPE is [`Answer::Escape`].
    fn ask(&self, request: &PromptRequest) -> Answer;

    /// Asks for a secret.
    fn secret(&self, request: &SecretRequest<'_>) -> Asked;

    /// Waits for a device, while it is not there.
    fn wait(&self, request: &WaitRequest<'_>) -> Wait;

    /// Says something along the way.
    fn notify(&self, tell: Tell<'_>);

    /// Says that `kernel` of the boot environment `environment` boots:
    /// a [`Told::Booting`] message by default, with the kernel
    /// highlighted.
    fn booting(&self, kernel: &str, environment: &str) {
        self.notify(Tell {
            told: Told::Booting,
            text: &booting_text(kernel, environment),
            highlight: Some(kernel),
        });
    }

    /// Asks the resume question. A front end that cannot ask it refuses,
    /// and the pool stays read-only.
    fn resume(&self, request: &ResumeRequest<'_>) -> Resume {
        let _ = request;
        Resume::Refuse
    }

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
/// for, the resume question is refused, and nothing is said.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tries_left_count_this_attempt_and_only_a_passphrase_skips() {
        let request = |attempt, tries| SecretRequest {
            subject: SecretSubject::Volume(VolumeRef {
                name: "root",
                device: "/dev/sda2",
            }),
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

    /// Records what it is told.
    #[derive(Default)]
    struct Told_(std::sync::Mutex<Vec<(Told, String, Option<String>)>>);

    impl Interaction for Told_ {
        fn ask(&self, _: &PromptRequest) -> Answer {
            Answer::Escape
        }

        fn secret(&self, _: &SecretRequest<'_>) -> Asked {
            Asked::Ended
        }

        fn wait(&self, _: &WaitRequest<'_>) -> Wait {
            Wait::TimedOut
        }

        fn notify(&self, tell: Tell<'_>) {
            self.0.lock().unwrap().push((
                tell.told,
                tell.text.to_owned(),
                tell.highlight.map(str::to_owned),
            ));
        }
    }

    #[test]
    fn a_boot_is_told_and_a_root_is_no_volume() {
        let told = Told_::default();
        told.booting("/boot/vmlinuz-6.1", "zroot/ROOT/void");
        assert_eq!(
            *told.0.lock().unwrap(),
            [(
                Told::Booting,
                "Booting /boot/vmlinuz-6.1 for zroot/ROOT/void ...".to_owned(),
                Some("/boot/vmlinuz-6.1".to_owned())
            )]
        );
        let volume = VolumeRef {
            name: "root",
            device: "/dev/sda2",
        };
        assert_eq!(SecretSubject::from(volume).volume(), Some(volume));
        assert_eq!(SecretSubject::EncryptionRoot("zroot").volume(), None);
    }

    #[test]
    fn no_one_answers_nothing_refuses_and_lends_no_console() {
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
            subject: volume.into(),
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
        assert_eq!(
            NoInteraction.resume(&ResumeRequest { pool: "tank" }),
            Resume::Refuse
        );
        let mut lent = None;
        NoInteraction.lease_console(&mut |runner| lent = Some(runner.is_some()));
        assert_eq!(lent, Some(false));
    }
}
