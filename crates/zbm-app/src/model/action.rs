//! What front ends ask for ([`Action`]) and what they are told
//! ([`Event`]).

use std::path::PathBuf;
use std::sync::Arc;

use zbm_crypt::OpenOptions;
use zbm_platform::secret::Secret;
use zbm_types::{
    Answer, DatasetName, DevicePath, FilesystemName, MappingName, PoolName, SnapshotName, Untrusted,
};

use super::{
    Change, DeviceLog, History, ImportOptions, Method, Notice, PackageDiff, Plan, Pool, Snapshot,
    SnapshotList, StageState,
};

/// What the name of the safety snapshot restoring files takes first starts
/// with: `<environment>@before-restore-<UTC time>`, the time as a new
/// snapshot is named (`%Y-%m-%d-%H%M%S`), so that each restore has its own
/// ([`restore_snapshot`](super::restore_snapshot)); one in the same second
/// as another ends with `-2`, `-3` and on.
pub const RESTORE_SNAPSHOT: &str = "before-restore";

/// What the menu asks the machine to do.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Action {
    /// Boot an environment with a kernel and a command line, its own when
    /// `None`.
    Boot {
        /// The environment.
        environment: DatasetName,
        /// The kernel version, as the environment's files name it.
        kernel: Option<Untrusted<String>>,
        /// The command line, as the pool or the user gives it.
        command_line: Option<Untrusted<String>>,
        /// Whether the countdown booted it, nobody having chosen: the
        /// countdown's boot tries no key source.
        countdown: bool,
    },
    /// Clone a snapshot into the new environment named, as
    /// [`Action::Clone`] does, and boot that environment once it is made.
    BootSnapshot(SnapshotName, DatasetName),
    /// Make an environment the `bootfs` of its pool.
    SetDefaultEnvironment(DatasetName),
    /// Make a kernel the default of its environment: its version, as the
    /// environment's files name it.
    SetDefaultKernel(DatasetName, Untrusted<String>),
    /// Let an environment inherit its default kernel again.
    InheritDefaultKernel(DatasetName),
    /// Save the command line an environment boots with, for every boot:
    /// `org.zfsbootmenu:commandline`, the line as it is; an empty one is
    /// [`Action::InheritCommandLine`].
    SetCommandLine(DatasetName, Untrusted<String>),
    /// Let an environment inherit its command line again.
    InheritCommandLine(DatasetName),
    /// Take the snapshot named.
    CreateSnapshot(SnapshotName),
    /// Roll an environment back to a snapshot, destroying the newer ones:
    /// ZFS keeps none of them, so nothing taken first could undo it.
    Rollback(SnapshotName),
    /// Destroy snapshots of one boot environment, all of them or none:
    /// ZFS keeps none of them, so nothing undoes it, and a clone or a hold
    /// of one stops it. The environment's bookmarks stay.
    Destroy(Vec<SnapshotName>),
    /// Clone a snapshot into the new environment named.
    Clone(SnapshotName, DatasetName),
    /// Clone a snapshot into the new environment named, and promote the
    /// clone.
    CloneAndPromote(SnapshotName, DatasetName),
    /// Copy a snapshot into the new, independent environment named.
    Duplicate(SnapshotName, DatasetName),
    /// The differences between a snapshot and another one or the live
    /// filesystem. Against the filesystem, `zfs diff` takes a snapshot of
    /// it, so the pool is imported read/write; two snapshots are compared
    /// on the pool as it is.
    Diff(SnapshotName, Option<SnapshotName>),
    /// Copy paths back from a snapshot into its environment, after a safety
    /// snapshot ([`RESTORE_SNAPSHOT`]); nothing is deleted. The menu offers
    /// it from a diff against the live filesystem.
    RestoreFiles {
        /// The snapshot.
        snapshot: SnapshotName,
        /// The paths, as the snapshot has them.
        paths: Vec<Untrusted<String>>,
    },
    /// Give an environment its boot tries back: mark it good.
    ResetBootTries(DatasetName),
    /// Seal a volume's TPM2 key to the PCRs as they are now.
    Reseal(String),
    /// A shell in a chroot of an environment, or of a snapshot read-only,
    /// opened on the console through the shell's gate, once its key is
    /// loaded ([`crate::chroot::open`]).
    Chroot(FilesystemName),
    /// Import a pool with the options given: one that is imported is
    /// exported first, which a rewind does whatever its access, and
    /// imported again; one that is not is imported read-only first, then
    /// again like one that is (ADR 0016, point 3).
    Import(PoolName, ImportOptions),
    /// Export an imported pool, which a scan then finds to import again.
    /// The pool of the default environment may be exported too: the
    /// snapshot read after it has no default.
    Export(PoolName),
    /// Unlock a volume with a method and, for typed methods, a secret.
    Unlock {
        /// The volume.
        volume: String,
        /// The method.
        method: Method,
        /// The secret typed, for typed methods; a clone of the action shares
        /// it.
        // A secret never crosses in the message: it follows as a packet
        // of its own (`zbm_app::remote`).
        #[serde(skip)]
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
    /// name without one (`zluks open`).
    Open {
        /// The device: a path or a tag such as `UUID=...`.
        device: String,
        /// The mapping name, if one is given.
        name: Option<MappingName>,
    },
    /// Open one LUKS volume as `name` with a key file and no other key
    /// method (`zluks open --key-file`).
    OpenWithKeyFile {
        /// The volume.
        device: DevicePath,
        /// The mapping name.
        name: MappingName,
        /// The key file, read where it is.
        key_file: PathBuf,
        /// How to open it beyond the key.
        options: OpenOptions,
    },
    /// Close the mappings ZFSBootMenu opened, or the one named
    /// (`zluks close`).
    Close(Option<MappingName>),
    /// The recovery shell.
    Shell,
    /// Reboot into the firmware's setup.
    FirmwareSetup,
    /// Reboot.
    Reboot,
    /// Power off.
    PowerOff,
    /// What a change would do, step by step, and what it would lose, keep
    /// and allow, without doing it: the review page and the dry run, which
    /// [`Event::Planned`] brings. It only reads ([`Action::is_read`]).
    Plan(Box<Action>),
    /// Look for the pools that could be imported, on every device, which
    /// can take seconds: [`Event::PoolsScanned`] brings them with the
    /// imported ones, and the snapshots read after it list them.
    ScanPools,
    /// The kernel's messages about a device of a pool, as the pool names
    /// it, and about those it stands on: [`Event::DeviceLogLoaded`] brings
    /// them.
    LoadDeviceLog(Untrusted<String>),
    /// Every snapshot of an environment, whose column opens:
    /// [`Event::SnapshotsLoaded`] brings them, and each snapshot of the
    /// machine read after it carries them again ([`Snapshot::loaded`]),
    /// until the snapshots of another environment are loaded.
    LoadSnapshots(DatasetName),
    /// The boots whose logs `zbm.save_logs` kept, which
    /// [`Event::HistoryLoaded`] brings; with the option off, none is read.
    LoadHistory,
}

impl Action {
    /// Whether the action only reads the machine, which it leaves as it
    /// was: what it reads comes in its own event, and the snapshot is not
    /// read again after it. The dry run of a change ([`Action::Plan`]) is
    /// one: it changes nothing, and a front end does not record it among
    /// the actions it asked for.
    pub fn is_read(&self) -> bool {
        matches!(
            self,
            Action::Plan(_)
                | Action::ScanPools
                | Action::LoadDeviceLog(_)
                | Action::LoadSnapshots(_)
                | Action::LoadHistory
        )
    }

    /// Whether the action answers what the machine asked
    /// ([`Event::UnlockRequested`], [`Event::Asked`]): it goes to the
    /// question's reply, as the action that asked waits for it.
    pub fn is_answer(&self) -> bool {
        matches!(
            self,
            Action::Unlock { .. } | Action::SkipUnlock(_) | Action::Answer(_)
        )
    }

    /// Whether the action takes the console from the menu: booting, which
    /// may ask for a passphrase, a snapshot's too, the recovery shell and
    /// the chroot, rebooting and powering off, and every import, which may
    /// ask about hibernation and must not be cancelled between the export
    /// and the import.
    pub fn needs_console(&self) -> bool {
        matches!(
            self,
            Action::Boot { .. }
                | Action::BootSnapshot(..)
                | Action::Import(..)
                | Action::Shell
                | Action::Chroot(_)
                | Action::Reboot
                | Action::PowerOff
                | Action::FirmwareSetup
        )
    }

    /// Whether the action is a change of package D: an import or an
    /// export, or a change of an environment or its snapshots, which
    /// imports the pool read/write first unless it is the diff of two
    /// snapshots ([`Action::needs_writable_pool`]), and loads the
    /// environment's key where it needs it.
    pub fn is_change(&self) -> bool {
        matches!(
            self,
            Action::Import(..)
                | Action::Export(_)
                | Action::SetDefaultEnvironment(_)
                | Action::SetDefaultKernel(..)
                | Action::InheritDefaultKernel(_)
                | Action::SetCommandLine(..)
                | Action::InheritCommandLine(_)
                | Action::CreateSnapshot(_)
                | Action::Rollback(_)
                | Action::Destroy(_)
                | Action::Clone(..)
                | Action::CloneAndPromote(..)
                | Action::Duplicate(..)
                | Action::Diff(..)
                | Action::RestoreFiles { .. }
        )
    }

    /// Whether the action has a plan, which a dry run shows
    /// ([`App::plan`](crate::App::plan)): a change but the differences of
    /// a snapshot, which changes nothing, and the boot of a snapshot,
    /// which is its clone's.
    pub fn has_plan(&self) -> bool {
        (self.is_change() && !matches!(self, Action::Diff(..)))
            || matches!(self, Action::BootSnapshot(..))
    }

    /// Whether the change writes its pool, which it imports read/write
    /// first: every change but the diff of two snapshots, which reads
    /// them, an import, which is how the pool is imported, and an export;
    /// and the boot of a snapshot, which clones it first.
    pub fn needs_writable_pool(&self) -> bool {
        let is_pool = matches!(
            self,
            Action::Diff(_, Some(_)) | Action::Import(..) | Action::Export(_)
        );
        matches!(self, Action::BootSnapshot(..)) || (self.is_change() && !is_pool)
    }

    /// Whether the locked menu keeps the action (ROADMAP.md SE9, ADR 0010):
    /// everything but the default boot, the countdown's or of `bootfs` with
    /// its own kernel and command line, what changes nothing (a diff, a dry
    /// run, the scan of the pools, a device's kernel messages, the snapshots
    /// of an environment, the boot history, which mounts the ESP read-only)
    /// and a pool imported read-only as this host, without a rewind.
    /// Unlocking a volume with its own secret, the recovery shell, which
    /// asks for the PIN itself, and powering off or rebooting stay open.
    pub fn is_kept_by_lock(&self, bootfs: Option<&DatasetName>) -> bool {
        match self {
            Action::Boot {
                environment,
                kernel,
                command_line,
                countdown,
            } => {
                let is_default =
                    bootfs == Some(environment) && kernel.is_none() && command_line.is_none();
                !(*countdown || is_default)
            }
            Action::Diff(..)
            | Action::Unlock { .. }
            | Action::SkipUnlock(_)
            | Action::Answer(_)
            | Action::UnlockAll
            | Action::Open { .. }
            | Action::Shell
            | Action::FirmwareSetup
            | Action::Reboot
            | Action::PowerOff
            | Action::Plan(_)
            | Action::ScanPools
            | Action::LoadDeviceLog(_)
            | Action::LoadSnapshots(_)
            | Action::LoadHistory => false,
            Action::Import(_, options) => options.is_kept_by_lock(),
            Action::BootSnapshot(..)
            | Action::SetDefaultEnvironment(_)
            | Action::SetDefaultKernel(..)
            | Action::InheritDefaultKernel(_)
            | Action::SetCommandLine(..)
            | Action::InheritCommandLine(_)
            | Action::CreateSnapshot(_)
            | Action::Rollback(_)
            | Action::Destroy(_)
            | Action::Clone(..)
            | Action::CloneAndPromote(..)
            | Action::Duplicate(..)
            | Action::RestoreFiles { .. }
            | Action::Export(_)
            | Action::ResetBootTries(_)
            | Action::Reseal(_)
            | Action::Chroot(_)
            | Action::OpenWithKeyFile { .. }
            | Action::Close(_) => true,
        }
    }

    /// The environment a change works on, whose key it loads and whose
    /// pool it writes, if it does ([`Action::needs_writable_pool`]); for a
    /// destroy, that of its first snapshot, as all must be of one.
    pub fn changed_environment(&self) -> Option<&DatasetName> {
        match self {
            Action::Destroy(snapshots) => snapshots.first().map(SnapshotName::dataset),
            Action::SetDefaultEnvironment(environment)
            | Action::SetDefaultKernel(environment, _)
            | Action::InheritDefaultKernel(environment)
            | Action::SetCommandLine(environment, _)
            | Action::InheritCommandLine(environment) => Some(environment),
            Action::BootSnapshot(snapshot, _)
            | Action::CreateSnapshot(snapshot)
            | Action::Rollback(snapshot)
            | Action::Clone(snapshot, _)
            | Action::CloneAndPromote(snapshot, _)
            | Action::Duplicate(snapshot, _)
            | Action::Diff(snapshot, _)
            | Action::RestoreFiles { snapshot, .. } => Some(snapshot.dataset()),
            _ => None,
        }
    }

    /// Whether the action takes the console on the machine `snapshot`
    /// describes: those of [`Action::needs_console`], a change that writes
    /// a pool imported read-only ([`Action::needs_writable_pool`]), as the
    /// read/write import may ask about hibernation, and one whose
    /// environment is locked, as the key may be asked for, but for a
    /// command line, which is set without it; and a duplicate whose target
    /// may go under a locked encryption root.
    pub fn needs_console_on(&self, snapshot: &super::Snapshot) -> bool {
        // The PIN is asked on the console.
        if self.needs_console()
            || (snapshot.menu_locked && self.is_kept_by_lock(snapshot.bootfs.as_ref()))
        {
            return true;
        }
        let Some(environment) = self.changed_environment() else {
            return false;
        };
        let pool = environment.pool();
        let read_only = self.needs_writable_pool()
            && snapshot
                .pools
                .iter()
                .any(|known| known.imported && known.name == pool && !known.writable);
        let keyless = matches!(
            self,
            Action::SetCommandLine(..) | Action::InheritCommandLine(_)
        );
        let locked = !keyless
            && snapshot.environments.iter().any(|known| {
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
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Event {
    /// The snapshot changed; boxed, as it is many times larger than the
    /// other events.
    Changed(Box<Snapshot>),
    /// What [`Action::ScanPools`] found: every pool, those imported and
    /// those that could be imported, which replace the snapshot's. The
    /// countdown goes on.
    PoolsScanned(Vec<Pool>),
    /// What [`Action::LoadDeviceLog`] found.
    DeviceLogLoaded(DeviceLog),
    /// The snapshots [`Action::LoadSnapshots`] read.
    SnapshotsLoaded(SnapshotList),
    /// [`Action::LoadSnapshots`] could not read the snapshots of the
    /// environment: their column shows none, and the [`Event::Finished`]
    /// after this says why, as [`Outcome::Failed`].
    SnapshotsUnread(DatasetName),
    /// The boot history [`Action::LoadHistory`] read.
    HistoryLoaded(History),
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
    /// What an [`Action::Plan`] asked for: the plan of the change.
    Planned {
        /// The change.
        action: Box<Action>,
        /// What it would do.
        plan: Plan,
    },
    /// The question asked last was answered otherwise, as by the device it
    /// waited for, or ended with the action that asked it: its dialog
    /// closes.
    Withdrawn,
    /// An action finished.
    Finished(Outcome),
    /// The menu ends: the machine boots, powers off or opens the shell.
    Leaving(String),
}

/// What start-up asks, besides secrets: [`Answer::Continue`] goes on, as
/// when the time runs out, and [`Answer::Escape`] stops.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
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
    /// Cancelled by the user before it ended; what it did meanwhile stays.
    Cancelled,
}

/// How unlocking a volume ended.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum UnlockOutcome {
    /// It is open.
    Opened,
    /// The user skipped it.
    Skipped,
    /// No key method opened it, for the reason given.
    Failed(String),
}
