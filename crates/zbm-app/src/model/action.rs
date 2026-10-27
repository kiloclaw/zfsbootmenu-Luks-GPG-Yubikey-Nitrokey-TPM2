//! What front ends ask for ([`Action`]) and what they are told
//! ([`Event`]).

use std::sync::Arc;

use zbm_platform::secret::Secret;
use zbm_types::{Answer, DatasetName, ImportPolicy, PoolName, SnapshotName};

use super::{Change, Method, Notice, PackageDiff, Snapshot, StageState};

/// The name of the safety snapshot restoring files takes first:
/// `<environment>@before-restore`.
pub const RESTORE_SNAPSHOT: &str = "before-restore";

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
        /// Whether the countdown booted it, nobody having chosen: the
        /// countdown's boot tries no key source.
        countdown: bool,
    },
    /// Boot a temporary clone of a snapshot.
    BootSnapshot(SnapshotName),
    /// Make an environment the `bootfs` of its pool.
    SetDefaultEnvironment(DatasetName),
    /// Make a kernel the default of its environment.
    SetDefaultKernel(DatasetName, String),
    /// Let an environment inherit its default kernel again.
    InheritDefaultKernel(DatasetName),
    /// Take the snapshot named.
    CreateSnapshot(SnapshotName),
    /// Roll an environment back to a snapshot, destroying the newer ones:
    /// ZFS keeps none of them, so nothing taken first could undo it.
    Rollback(SnapshotName),
    /// Clone a snapshot into the new environment named.
    Clone(SnapshotName, DatasetName),
    /// Clone a snapshot into the new environment named, and promote the
    /// clone.
    CloneAndPromote(SnapshotName, DatasetName),
    /// Copy a snapshot into the new, independent environment named.
    Duplicate(SnapshotName, DatasetName),
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
    /// Seal a volume's TPM2 key to the PCRs as they are now.
    Reseal(String),
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
        /// The secret typed, for typed methods; a clone of the action shares
        /// it.
        secret: Option<Arc<Secret>>,
    },
    /// Leave a volume locked, or try its next key method.
    SkipUnlock(String),
    /// Answer what start-up asked ([`Event::Asked`]): go on
    /// ([`Answer::Continue`]), or stop ([`Answer::Escape`]).
    Answer(Answer),
    /// Unlock every LUKS volume the configuration and discovery find, with
    /// their key methods and prompts, as start-up does (`zluks unlock`).
    UnlockAll,
    /// Open one LUKS volume as a mapping of the name given, or its default
    /// name when empty (`zluks open`).
    Open {
        /// The device: a path or a tag such as `UUID=...`.
        device: String,
        /// The mapping name, or empty.
        name: String,
    },
    /// Close the mappings ZFSBootMenu opened, or the one named
    /// (`zluks close`).
    Close(Option<String>),
    /// The recovery shell.
    Shell,
    /// Reboot into the firmware's setup.
    FirmwareSetup,
    /// Reboot.
    Reboot,
    /// Power off.
    PowerOff,
}

impl Action {
    /// Whether the action takes the console from the menu: booting, which
    /// may ask for a passphrase, the recovery shell, rebooting and powering
    /// off.
    pub fn needs_console(&self) -> bool {
        matches!(
            self,
            Action::Boot { .. }
                | Action::Shell
                | Action::Reboot
                | Action::PowerOff
                | Action::FirmwareSetup
        )
    }

    /// Whether the action changes a pool: package D, which imports the
    /// pool read/write first and loads the environment's key.
    pub fn is_change(&self) -> bool {
        matches!(
            self,
            Action::SetDefaultEnvironment(_)
                | Action::CreateSnapshot(_)
                | Action::Rollback(_)
                | Action::Clone(..)
                | Action::CloneAndPromote(..)
                | Action::Duplicate(..)
                | Action::Diff(..)
        )
    }

    /// The environment a change works on, whose pool it writes and whose
    /// key it loads.
    pub fn changed_environment(&self) -> Option<&DatasetName> {
        match self {
            Action::SetDefaultEnvironment(environment) => Some(environment),
            Action::CreateSnapshot(snapshot)
            | Action::Rollback(snapshot)
            | Action::Clone(snapshot, _)
            | Action::CloneAndPromote(snapshot, _)
            | Action::Duplicate(snapshot, _)
            | Action::Diff(snapshot, _) => Some(snapshot.dataset()),
            _ => None,
        }
    }

    /// Whether the action takes the console on the machine `snapshot`
    /// describes: those of [`Action::needs_console`], and a change whose
    /// pool is imported read-only or whose environment is locked, as the
    /// read/write import may ask about hibernation and the key may be
    /// asked for; and a duplicate whose target may go under a locked
    /// encryption root.
    pub fn needs_console_on(&self, snapshot: &super::Snapshot) -> bool {
        if self.needs_console() {
            return true;
        }
        let Some(environment) = self.changed_environment() else {
            return false;
        };
        let pool = environment.pool();
        let read_only = snapshot
            .pools
            .iter()
            .any(|known| known.name == pool && !known.writable);
        let locked = snapshot.environments.iter().any(|known| {
            known.name.as_str() == environment.as_str()
                && known.encryption == super::Encryption::Locked
        });
        read_only || locked || self.target_may_be_locked(snapshot)
    }

    /// Whether a duplicate's target may go under a locked encryption root:
    /// the duplicate loads the key of the target's parent. The snapshot
    /// knows that key when the parent is an environment, or holds an
    /// unencrypted source, as ZFS keeps the parents of an unencrypted
    /// dataset unencrypted.
    fn target_may_be_locked(&self, snapshot: &super::Snapshot) -> bool {
        let Action::Duplicate(source, target) = self else {
            return false;
        };
        let Some((parent, _)) = target.as_str().rsplit_once('/') else {
            return false;
        };
        let encryption = |name: &str| {
            snapshot
                .environments
                .iter()
                .find(|known| known.name.as_str() == name)
                .map(|known| &known.encryption)
        };
        if let Some(known) = encryption(parent) {
            return *known == super::Encryption::Locked;
        }
        let source = source.dataset().as_str();
        let holds_source = source
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'));
        !(holds_source && encryption(source) == Some(&super::Encryption::None))
    }
}

/// What the machine tells the menu.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// The snapshot changed.
    Changed(Snapshot),
    /// A stage of start-up started, named for people.
    StageStarted(String),
    /// A stage of start-up ended.
    StageFinished {
        /// The stage, named for people.
        stage: String,
        /// How it ended.
        state: StageState,
    },
    /// An unlocking run ended for a volume.
    UnlockFinished {
        /// The volume: its mapping name.
        volume: String,
        /// How it ended.
        outcome: UnlockOutcome,
    },
    /// A volume needs a secret or a device.
    UnlockRequested {
        /// The volume.
        volume: String,
        /// The method being tried.
        method: Method,
        /// Tries left for a typed method, if limited.
        tries_left: Option<u32>,
        /// Why this method is asked, when another one failed or the last
        /// try was wrong: `the TPM kept the key: …`.
        notice: Option<Notice>,
        /// For a method with nothing to type: how many seconds start-up
        /// waits for its device.
        wait: Option<u64>,
    },
    /// Start-up asks whether to go on, counting `seconds` down when it
    /// does so by itself.
    Asked {
        /// What it asks.
        question: Question,
        /// The seconds until it goes on by itself; `None`: it waits.
        seconds: Option<u64>,
    },
    /// What start-up is doing that may take a while, for people:
    /// `Unlocking /dev/sda2 ...`, `Touch the security key when it blinks.`
    /// The next event replaces it.
    Progress(String),
    /// A volume was opened without its TPM2 key, which the TPM kept: the
    /// menu offers to seal it again.
    ResealOffered(String),
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
        /// The packages that changed; `None` when no package database was
        /// found.
        packages: Option<PackageDiff>,
    },
    /// An action finished.
    Finished(Outcome),
    /// The menu ends: the machine boots, powers off or opens the shell.
    Leaving(String),
}

/// What start-up asks, besides secrets: [`Answer::Continue`] goes on, as
/// when the time runs out, and [`Answer::Escape`] stops.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Question {
    /// No passphrase opened a volume this round: try again, or skip it.
    RetryUnlock {
        /// The volume.
        volume: String,
    },
    /// A LUKS device of the configuration is missing: look again, or skip
    /// it.
    MissingVolume {
        /// The device, as the configuration names it.
        device: String,
    },
    /// Devices of `zbm.wait_for` are missing: look again, or stop waiting.
    MissingDevices,
    /// A pool did not import, or none did: try again, or open the recovery
    /// shell.
    ImportFailed {
        /// The pool start-up wanted, if one.
        pool: Option<String>,
    },
    /// A pool has features this build of ZFS does not support: go on.
    Unsupported {
        /// What to do about it.
        advice: String,
    },
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

/// How unlocking a volume ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnlockOutcome {
    /// It is open.
    Opened,
    /// The user skipped it.
    Skipped,
    /// No key method opened it, for the reason given.
    Failed(String),
}
