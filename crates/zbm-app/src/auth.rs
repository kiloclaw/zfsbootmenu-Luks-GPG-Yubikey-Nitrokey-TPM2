//! The lock of the menu and the one gate to a shell (ROADMAP.md SE9,
//! [ADR 0010](../../../docs/dev/adr/0010-locked-menu.md)).
//!
//! An image may be locked: it holds the Argon2id hash of a PIN in
//! [`LOCK_FILE`]. Then everything but the default boot needs someone who
//! proved themselves: who typed the PIN, or a passphrase that unlocked a
//! volume this boot. The [`Authority`] of a process keeps that proof for a
//! while ([`IDLE`]); without a lock, everyone is authorised.
//!
//! [`ShellGate::open`] is the only way to an interactive shell, and it takes
//! an [`Authorized`], which only this module mints: from the authority, or
//! at once where there is no lock. The supervisor, which never holds a
//! proof, takes a [`RecoveryPolicy`] instead: the shell, or the prompt that
//! asks for the PIN first.

use std::io;
use std::path::Path;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use zbm_platform::{Files, Output, RealFiles, Runner, Secret, System};
use zbm_state::State;
use zbm_tools::programs::ShellRoot;
use zbm_types::FilesystemName;

use crate::keyholder::Keyholders;
use crate::model::{Event, Outcome};

/// Where a locked image keeps the hash of its PIN.
pub const LOCK_FILE: &str = zbm_config::conf::LOCK;

/// How long a proof lasts once nothing was done with it.
pub const IDLE: Duration = Duration::from_secs(300);

/// The longest wait before a PIN is checked, after wrong ones.
pub const LONGEST_DELAY: Duration = Duration::from_secs(30);

/// Where the keyholder counts the wrong PINs of this boot, in the state
/// directory.
const FAILURES: &str = "lock-failures";

/// The lock of an image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Lock {
    /// No lock: everyone is authorised.
    Off,
    /// Locked with the PIN whose hash this is, in the PHC string format
    /// (`$argon2id$v=19$m=…`); empty when the lock file could not be read,
    /// which no PIN matches.
    On(String),
}

impl Lock {
    /// The lock of the image whose files are `files`: on when
    /// [`LOCK_FILE`] exists, even when it cannot be read.
    pub fn of(files: &dyn Files) -> Lock {
        Lock::at(files, Path::new(LOCK_FILE))
    }

    /// The lock that the file `path` holds, as [`Lock::of`].
    pub fn at(files: &dyn Files, path: &Path) -> Lock {
        if !files.exists(path) {
            return Lock::Off;
        }
        let hash = files.read_to_string(path).unwrap_or_default();
        Lock::On(hash.trim().to_owned())
    }

    /// Whether the image is locked.
    pub fn is_on(&self) -> bool {
        matches!(self, Lock::On(_))
    }
}

/// The proof that whoever asks may do what the lock keeps: the shell, the
/// command line, changes. Only this module makes one.
#[derive(Debug)]
pub struct Authorized {
    _private: (),
}

/// The proof that someone typed the PIN or a passphrase that unlocked a
/// volume, which this crate alone makes, when it checked it.
#[derive(Debug)]
pub struct Verified {
    _private: (),
}

impl Verified {
    /// A proof, for the checks of this crate.
    pub(crate) fn new() -> Verified {
        Verified { _private: () }
    }
}

/// The lock of a process and the proof it holds: since when, by the
/// clock's [`zbm_platform::Clock::now`], it was last used.
#[derive(Debug)]
pub struct Authority {
    lock: Lock,
    last_used: Mutex<Option<Duration>>,
}

impl Authority {
    /// The authority of an image locked as `lock`, without a proof.
    pub fn new(lock: Lock) -> Authority {
        Authority {
            lock,
            last_used: Mutex::new(None),
        }
    }

    /// Whether what the lock keeps needs a proof first: the image is locked,
    /// no person opened a volume this boot (`attended`), and the proof, if
    /// any, is older than [`IDLE`] at `now`.
    pub fn is_locked(&self, attended: bool, now: Duration) -> bool {
        self.lock.is_on() && !attended && !self.has_proof(now)
    }

    /// Whether a proof was used within [`IDLE`] before `now`.
    fn has_proof(&self, now: Duration) -> bool {
        let last_used = *self
            .last_used
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        last_used.is_some_and(|at| now.saturating_sub(at) < IDLE)
    }

    /// The authorisation at `now`, which uses the proof: at once without a
    /// lock, or when a person opened a volume this boot (`attended`, which
    /// [`zbm_state::State::has_attended_unlock`] says), else while a proof
    /// lasts.
    pub fn authorized(&self, attended: bool, now: Duration) -> Option<Authorized> {
        if !self.lock.is_on() || attended {
            return Some(Authorized { _private: () });
        }
        let mut last_used = self
            .last_used
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let lasts = last_used.is_some_and(|at| now.saturating_sub(at) < IDLE);
        if !lasts {
            *last_used = None;
            return None;
        }
        *last_used = Some(now);
        Some(Authorized { _private: () })
    }

    /// Keeps the proof `verified`, made at `now`.
    pub fn prove(&self, _verified: Verified, now: Duration) {
        *self
            .last_used
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(now);
    }

    /// Forgets the proof: what the lock keeps needs a new one.
    pub fn forget(&self) {
        *self
            .last_used
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// The authority of this process, for the image it runs from.
pub fn process() -> &'static Authority {
    static AUTHORITY: OnceLock<Authority> = OnceLock::new();
    AUTHORITY.get_or_init(|| Authority::new(Lock::of(&RealFiles)))
}

/// The way to an interactive shell, opened by an authorisation.
#[derive(Debug)]
pub struct ShellGate {
    _private: (),
}

impl ShellGate {
    /// The gate, opened by `authorized`.
    pub fn open(_authorized: &Authorized) -> ShellGate {
        ShellGate { _private: () }
    }

    /// Runs the interactive shell the runtime chose on the console with
    /// `runner`, until it ends; the gate is closed again.
    ///
    /// # Errors
    ///
    /// The error that kept the shell from starting.
    pub fn run(self, runner: &dyn Runner) -> io::Result<Output> {
        Self::start(runner, ShellRoot::Image)
    }

    /// Runs `zfs-chroot` for `filesystem` on the console with `runner`,
    /// which opens a shell in a chroot of it, until it ends; the gate is
    /// closed again.
    ///
    /// # Errors
    ///
    /// The error that kept `zfs-chroot` from starting.
    pub fn chroot(self, runner: &dyn Runner, filesystem: &FilesystemName) -> io::Result<Output> {
        Self::start(runner, ShellRoot::Chroot(filesystem))
    }

    /// The gate's one start of a shell, in `root`, which only an open gate
    /// reaches.
    fn start(runner: &dyn Runner, root: ShellRoot<'_>) -> io::Result<Output> {
        zbm_tools::programs::interactive_shell(runner, root)
    }
}

/// The wait before a PIN is checked, after `failures` wrong ones: none
/// before the first, then a second, doubled after each, up to
/// [`LONGEST_DELAY`].
pub fn delay(failures: u32) -> Duration {
    let Some(doublings) = failures.checked_sub(1) else {
        return Duration::ZERO;
    };
    let seconds = 1_u64.checked_shl(doublings).unwrap_or(u64::MAX);
    Duration::from_secs(seconds).min(LONGEST_DELAY)
}

/// The keyholder's check of `pin` against the image's `lock` (ROADMAP.md
/// SE9): after the [`delay`] that the wrong PINs of this boot, which
/// `state` counts, call for, whether it is the PIN. A right one sets the
/// count back. Without a lock there is no PIN to be.
pub fn is_image_pin(system: &System<'_>, state: &State<'_>, lock: &Lock, pin: &Secret) -> bool {
    let Lock::On(hash) = lock else {
        return false;
    };
    let count = state.base().join(FAILURES);
    let failures = system
        .files
        .read_to_string(&count)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(0);
    system.clock.sleep(delay(failures));
    let is_pin = zbm_crypt::pin::is_pin_of(hash, pin);
    let failures = if is_pin {
        0
    } else {
        failures.saturating_add(1)
    };
    if let Err(error) = system
        .files
        .write(&count, format!("{failures}\n").as_bytes())
    {
        tracing::warn!(%error, "unable to count the wrong PINs");
    }
    if is_pin {
        tracing::info!("the PIN of the locked menu was typed");
    } else {
        tracing::warn!(failures, "a wrong PIN for the locked menu");
    }
    is_pin
}

/// Has a keyholder of `keyholders` check `pin` (ROADMAP.md SE9): when it
/// is the image's PIN, `authority` keeps the proof, made at `now`. Whether
/// it was.
///
/// # Errors
///
/// When no keyholder could check it.
pub fn check_pin(
    authority: &Authority,
    keyholders: &dyn Keyholders,
    pin: &Secret,
    now: Duration,
) -> io::Result<bool> {
    let verified = crate::keyholder::verify(keyholders, pin)?;
    if verified {
        authority.prove(Verified::new(), now);
    }
    Ok(verified)
}

/// Opens the recovery shell on the console with `runner`: through its gate
/// when `authority` authorises it at `now`, as a person who opened a volume
/// of `state` is, else through the prompt that asks for the PIN first
/// (`zbm lock shell`). Once the shell ended, the proof is forgotten: the
/// menu is locked again.
///
/// # Errors
///
/// The error that kept the shell or the prompt from starting.
pub fn recovery_shell(
    authority: &Authority,
    state: &State<'_>,
    runner: &dyn Runner,
    now: Duration,
) -> io::Result<Output> {
    let Some(authorized) = authority.authorized(state.has_attended_unlock(), now) else {
        return zbm_tools::programs::locked_shell(runner);
    };
    let ran = ShellGate::open(&authorized).run(runner);
    authority.forget();
    ran
}

/// Opens a shell in a chroot of `filesystem` on the console with
/// `runner`, through the shell's gate, when `authority` authorises it at
/// `now`, as a person who opened a volume of `state` is; `None`, running
/// nothing, when it does not. Once the shell ended, the proof is
/// forgotten, as after the recovery shell.
pub fn chroot(
    authority: &Authority,
    state: &State<'_>,
    runner: &dyn Runner,
    now: Duration,
    filesystem: &FilesystemName,
) -> Option<io::Result<Output>> {
    let authorized = authority.authorized(state.has_attended_unlock(), now)?;
    let ran = ShellGate::open(&authorized).chroot(runner, filesystem);
    authority.forget();
    Some(ran)
}

/// The Argon2id hash of `pin`, salted from the kernel's random numbers
/// through `files`, as an image locked with it holds it: what
/// `zbm lock hash` prints for `zfsbootmenu_lock_hash`.
///
/// # Errors
///
/// Why it could not be hashed: a PIN shorter than
/// [`zbm_crypt::pin::MIN_LENGTH`], or no salt.
pub fn hash_pin(files: &dyn Files, pin: &Secret) -> Result<String, String> {
    let random = files
        .read_prefix(Path::new("/dev/urandom"), zbm_crypt::pin::SALT)
        .map_err(|error| format!("unable to read a salt: {error}"))?;
    let salt: [u8; zbm_crypt::pin::SALT] = random
        .try_into()
        .map_err(|_| "unable to read a whole salt".to_owned())?;
    zbm_crypt::pin::hash(pin, &salt).map_err(|error| error.to_string())
}

/// What the session asks on the console before an action the locked menu
/// keeps.
pub const LOCKED_PROMPT: &str = "This ZFSBootMenu is locked. PIN (blank to go back): ";

/// How an action that the locked menu kept ended: refused.
pub fn refused() -> Vec<Event> {
    vec![Event::Finished(Outcome::Failed {
        what: "this ZFSBootMenu is locked".to_owned(),
        why: vec!["it needs the PIN, or a passphrase that opened a volume this boot".to_owned()],
        next: Vec::new(),
    })]
}

/// What the supervisor opens when the start-up program ended.
#[derive(Debug)]
pub enum RecoveryPolicy {
    /// The recovery shell, through its gate.
    Open(ShellGate),
    /// The prompt that asks for the PIN, then opens the shell.
    Locked,
}

impl RecoveryPolicy {
    /// The policy of `authority` at `now`, to which no person's unlock
    /// counts: the supervisor reads no state.
    pub fn of(authority: &Authority, now: Duration) -> RecoveryPolicy {
        match authority.authorized(false, now) {
            Some(authorized) => RecoveryPolicy::Open(ShellGate::open(&authorized)),
            None => RecoveryPolicy::Locked,
        }
    }

    /// The policy of a process of an image locked as `lock`, which holds no
    /// proof, as the supervisor.
    pub fn of_image(lock: &Lock) -> RecoveryPolicy {
        RecoveryPolicy::of(&Authority::new(lock.clone()), Duration::ZERO)
    }
}

#[cfg(test)]
mod tests;
