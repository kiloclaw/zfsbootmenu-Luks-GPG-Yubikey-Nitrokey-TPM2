//! What the menu knows about the machine: the snapshot it reads, the
//! actions it asks for and the events it receives.
//!
//! These are the lists of `docs/dev/menu-design/interfaces.md`, proposed for
//! `zbm-app` (R4). The prototype answers them with
//! [`FakeMachine`](super::demo::FakeMachine); R4 moves them into `zbm-app`,
//! and the menu then imports them from there.

mod diff;
mod health;

use zbm_app::Encryption;
use zbm_platform::secret::Secret;
use zbm_types::{DatasetName, ImportPolicy, PoolName, SnapshotName};

pub use self::diff::{Change, ChangeKind};
pub use self::health::{Check, Fallback, Target, Tries, Verdict, worst};

/// The machine, as every view reads it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    /// The stages of start-up.
    pub startup: Vec<Stage>,
    /// The countdown before the default environment boots.
    pub timeout: Timeout,
    /// The pools.
    pub pools: Vec<Pool>,
    /// The boot environments.
    pub environments: Vec<Environment>,
    /// The LUKS volumes.
    pub volumes: Vec<Volume>,
    /// The log since start.
    pub log: Log,
    /// The time the snapshot describes, in seconds since the epoch, for ages.
    pub now: u64,
    /// The environment the running system asked to boot once
    /// (`zbm next-boot`); start-up read and deleted the request.
    pub next_boot: Option<DatasetName>,
    /// The default environment used up its boot tries: what boots instead.
    pub fallback: Option<Fallback>,
}

/// One stage of start-up.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stage {
    /// What it does, for people.
    pub name: String,
    /// How far it got.
    pub state: StageState,
}

/// How far a stage of start-up got.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StageState {
    /// Not started.
    Pending,
    /// Running.
    Running,
    /// Done.
    Done,
    /// Failed, for the reason given.
    Failed(String),
    /// Not needed on this machine.
    Skipped,
}

/// The countdown before the default environment boots.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Timeout {
    /// Boot after this many seconds.
    Seconds(u32),
    /// Wait for the user.
    #[default]
    Wait,
    /// Boot at once.
    Skip,
}

/// The health of a pool, as `zpool status` names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Health {
    /// Every device works.
    Online,
    /// Redundancy is lost, data is there.
    Degraded,
    /// Data is missing.
    Faulted,
    /// Devices cannot be opened.
    Unavailable,
}

impl Health {
    /// The word `zpool status` uses.
    pub fn as_str(self) -> &'static str {
        match self {
            Health::Online => "ONLINE",
            Health::Degraded => "DEGRADED",
            Health::Faulted => "FAULTED",
            Health::Unavailable => "UNAVAIL",
        }
    }
}

/// A pool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pool {
    /// Its name.
    pub name: PoolName,
    /// Its health.
    pub health: Health,
    /// Whether it is imported read/write.
    pub writable: bool,
    /// Whether it is imported.
    pub imported: bool,
    /// The host ID it was last imported by, when it is another host's.
    pub foreign_hostid: Option<String>,
    /// Whether it has a checkpoint to rewind to.
    pub checkpoint: bool,
}

/// A boot environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Environment {
    /// The filesystem.
    pub name: DatasetName,
    /// Whether it is the `bootfs` of its pool.
    pub is_default: bool,
    /// Its encryption.
    pub encryption: Encryption,
    /// Its kernels, newest first.
    pub kernels: Vec<Kernel>,
    /// Its command line.
    pub command_line: CommandLine,
    /// Its snapshots, newest first; loaded on demand in R5 (see
    /// `docs/dev/menu-design/interfaces.md`), held in full by the prototype.
    pub snapshots: Vec<SnapshotInfo>,
    /// Whether it will boot: the checks, empty when none ran.
    pub checks: Vec<Check>,
    /// Its boot counting, when `zbm.boot_tries` is set.
    pub tries: Option<Tries>,
}

impl Environment {
    /// The kernel that boots unless another is chosen: the default, else
    /// the newest.
    pub fn boot_kernel(&self) -> Option<&Kernel> {
        self.kernels
            .iter()
            .find(|kernel| kernel.is_default)
            .or_else(|| self.kernels.first())
    }
}

/// A kernel of a boot environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Kernel {
    /// Its version.
    pub version: String,
    /// Whether `org.zfsbootmenu:kernel` names it.
    pub is_default: bool,
}

/// The command line of a boot environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandLine {
    /// The command line.
    pub value: String,
    /// Where it was set.
    pub origin: Origin,
}

/// Where a property was set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Origin {
    /// On the dataset itself.
    Local,
    /// On the dataset named.
    Inherited(DatasetName),
    /// Nowhere: the default.
    Default,
}

/// A snapshot of a boot environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotInfo {
    /// Its name.
    pub name: SnapshotName,
    /// When it was taken, in seconds since the epoch.
    pub created: u64,
    /// The space only it holds, in bytes.
    pub used: u64,
}

/// A LUKS volume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Volume {
    /// The name of its mapping.
    pub name: String,
    /// Its device.
    pub device: String,
    /// Its state.
    pub state: VolumeState,
    /// The key methods that apply, in the order they are tried.
    pub methods: Vec<Method>,
    /// The pool it holds a device of, when known.
    pub pool: Option<PoolName>,
}

/// The state of a LUKS volume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VolumeState {
    /// Not open.
    Locked,
    /// Open.
    Open,
    /// Every method failed, for the reason given.
    Failed(String),
    /// Skipped by the user.
    Skipped,
}

/// A way to unlock a volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    /// A passphrase.
    Passphrase,
    /// A TPM2 token with a PIN.
    Tpm2Pin,
    /// A FIDO2 security key.
    Fido2,
    /// A YubiKey in challenge-response mode.
    YubiKey,
    /// A key file encrypted with GnuPG, with its passphrase.
    GnupgKeyFile,
    /// A key file on a device that has to be plugged in.
    KeyDevice,
}

impl Method {
    /// How the menu names it.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Passphrase => "passphrase",
            Method::Tpm2Pin => "TPM2 PIN",
            Method::Fido2 => "security key",
            Method::YubiKey => "YubiKey",
            Method::GnupgKeyFile => "key file passphrase",
            Method::KeyDevice => "key device",
        }
    }

    /// Whether the user types a secret for it.
    pub fn is_typed(self) -> bool {
        matches!(
            self,
            Method::Passphrase | Method::Tpm2Pin | Method::GnupgKeyFile
        )
    }
}

/// The log since start: R5 loads the entries with a `LoadLog` action; the
/// prototype holds them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Log {
    /// The entries, oldest first.
    pub entries: Vec<LogEntry>,
}

impl Log {
    /// The number of entries of `level`.
    pub fn count(&self, level: Level) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.level == level)
            .count()
    }
}

/// An entry of the log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogEntry {
    /// Its level.
    pub level: Level,
    /// What it says.
    pub text: String,
}

/// The level of a log entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    /// Information.
    Info,
    /// A warning.
    Warning,
    /// An error.
    Error,
}

/// What the menu asks the machine to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// Boot an environment with a kernel and a command line, its own when
    /// `None`.
    Boot {
        /// The environment.
        environment: DatasetName,
        /// The kernel version.
        kernel: Option<String>,
        /// The command line.
        command_line: Option<String>,
    },
    /// Boot a temporary clone of a snapshot.
    BootSnapshot(SnapshotName),
    /// Make an environment the `bootfs` of its pool.
    SetDefaultEnvironment(DatasetName),
    /// Make a kernel the default of its environment.
    SetDefaultKernel(DatasetName, String),
    /// Let an environment inherit its default kernel again.
    InheritDefaultKernel(DatasetName),
    /// Take a snapshot.
    CreateSnapshot(DatasetName),
    /// Roll an environment back to a snapshot, after a safety snapshot when
    /// `safety` is set.
    Rollback {
        /// The snapshot.
        snapshot: SnapshotName,
        /// Whether to take a safety snapshot first.
        safety: bool,
    },
    /// Clone a snapshot into a new environment.
    Clone(SnapshotName),
    /// Clone a snapshot and promote the clone.
    CloneAndPromote(SnapshotName),
    /// Copy a snapshot into an independent environment.
    Duplicate(SnapshotName),
    /// The differences between a snapshot and another one or the live
    /// filesystem.
    Diff(SnapshotName, Option<SnapshotName>),
    /// Copy paths back from a snapshot into its environment, after a safety
    /// snapshot.
    RestoreFiles {
        /// The snapshot.
        snapshot: SnapshotName,
        /// The paths, as the snapshot has them.
        paths: Vec<String>,
    },
    /// Give an environment its boot tries back: mark it good.
    ResetBootTries(DatasetName),
    /// A shell in a chroot of an environment, or of a snapshot read-only.
    Chroot(String),
    /// Import a pool read/write again.
    ImportReadWrite(PoolName),
    /// Rewind a pool to its checkpoint.
    RewindCheckpoint(PoolName),
    /// Import a pool with a policy.
    Import(PoolName, ImportPolicy),
    /// Unlock a volume with a method and, for typed methods, a secret.
    Unlock {
        /// The volume.
        volume: String,
        /// The method.
        method: Method,
        /// The secret typed, for typed methods.
        secret: Option<Secret>,
    },
    /// Leave a volume locked.
    SkipUnlock(String),
    /// The recovery shell.
    Shell,
    /// Reboot into the firmware's setup.
    FirmwareSetup,
    /// Reboot.
    Reboot,
    /// Power off.
    PowerOff,
}

/// What the machine tells the menu.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// The snapshot changed.
    Changed(Snapshot),
    /// A volume needs a secret or a device.
    UnlockRequested {
        /// The volume.
        volume: String,
        /// The method being tried.
        method: Method,
        /// Tries left for a typed method, if limited.
        tries_left: Option<u32>,
    },
    /// A pool did not import.
    ImportProblem {
        /// The pool.
        pool: PoolName,
        /// Why, for people.
        reason: String,
    },
    /// The differences a [`Action::Diff`] asked for.
    DiffReady {
        /// The snapshot.
        from: SnapshotName,
        /// The other snapshot; `None`: the live filesystem.
        to: Option<SnapshotName>,
        /// What changed.
        changes: Vec<Change>,
    },
    /// An action finished.
    Finished(Outcome),
    /// The menu ends: the machine boots, powers off or opens the shell.
    Leaving(String),
}

/// How an action ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Done, with what happened.
    Done(String),
    /// Done, with something to note.
    Warning(String),
    /// Failed: what failed, why, and the next steps.
    Failed {
        /// What failed.
        what: String,
        /// Why, from the error's causes.
        why: Vec<String>,
        /// What the user can do next.
        next: Vec<String>,
    },
}

/// The pool with the worst health, if any is not online.
pub fn worst_pool(snapshot: &Snapshot) -> Option<&Pool> {
    snapshot
        .pools
        .iter()
        .filter(|pool| pool.health != Health::Online)
        .max_by_key(|pool| pool.health)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn pool(name: &str, health: Health) -> Pool {
        Pool {
            name: name.parse().unwrap(),
            health,
            writable: false,
            imported: true,
            foreign_hostid: None,
            checkpoint: false,
        }
    }

    #[test]
    fn the_worst_pool_is_the_least_healthy() {
        let snapshot = Snapshot {
            pools: vec![
                pool("a", Health::Online),
                pool("b", Health::Faulted),
                pool("c", Health::Degraded),
            ],
            ..Snapshot::default()
        };

        assert_eq!(worst_pool(&snapshot).map(|p| p.name.as_str()), Some("b"));
        assert_eq!(worst_pool(&Snapshot::default()), None);
        assert_eq!(Health::Unavailable.as_str(), "UNAVAIL");
    }

    #[test]
    fn the_boot_kernel_is_the_default_or_the_newest() {
        let mut environment = Environment {
            name: "z/ROOT/a".parse().unwrap(),
            is_default: true,
            encryption: Encryption::None,
            kernels: vec![
                Kernel {
                    version: "6.9".into(),
                    is_default: false,
                },
                Kernel {
                    version: "6.8".into(),
                    is_default: true,
                },
            ],
            command_line: CommandLine {
                value: String::new(),
                origin: Origin::Default,
            },
            snapshots: Vec::new(),
            checks: Vec::new(),
            tries: None,
        };

        assert_eq!(
            environment.boot_kernel().map(|k| k.version.as_str()),
            Some("6.8")
        );
        environment.kernels[1].is_default = false;
        assert_eq!(
            environment.boot_kernel().map(|k| k.version.as_str()),
            Some("6.9")
        );
        assert!(Method::Tpm2Pin.is_typed() && !Method::Fido2.is_typed());
        assert_eq!(Method::GnupgKeyFile.as_str(), "key file passphrase");
    }
}
