//! What front ends ask for ([`Action`]) and what they are told
//! ([`Event`]).

use zbm_platform::secret::Secret;
use zbm_types::{DatasetName, ImportPolicy, PoolName, SnapshotName};

use super::{Change, Method, Notice, PackageDiff, Snapshot, StageState};

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
        /// The secret typed, for typed methods.
        secret: Option<Secret>,
    },
    /// Leave a volume locked.
    SkipUnlock(String),
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
    },
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
