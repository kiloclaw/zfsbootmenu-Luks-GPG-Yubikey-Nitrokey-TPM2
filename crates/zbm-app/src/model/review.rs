//! What a change loses, what it keeps, what undoes it, what is safer, and
//! what stops it: the review page of a change shows it before it asks for
//! the target's name, and the dry run shows it with the steps (ADR 0016,
//! points 12 to 15). `zbm-app` reads it from the machine
//! ([`App::plan`](crate::App::plan)).
//!
//! A read that fails leaves what it gives unknown, `None`: the change is
//! still possible. Only a [`Stop`] makes the plan fail, as ZFS would
//! refuse the change.
//!
//! Clone names, hold tags and host names come from pools that whoever
//! made them named: they are [`Untrusted`].

use zbm_types::{DatasetName, PoolName, SnapshotName, Untrusted};

use super::{Action, PoolHost};

/// What a change would lose, keep and allow, as the machine is now.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Review {
    /// What it loses, in the order ZFS made it; none for a change that
    /// loses nothing.
    pub loses: Vec<Loss>,
    /// The space, in bytes, that destroying the snapshots lost frees
    /// together (`zfs destroy -n -v -p`); `None` when it could not be
    /// counted, as when a clone stops the change.
    pub freed: Option<u64>,
    /// What it keeps that a reader might fear for.
    pub keeps: Vec<Keep>,
    /// The action that undoes it; `None` when nothing does.
    pub undo: Option<Box<Action>>,
    /// An action that gives most of what it gives, and loses nothing.
    pub safer: Option<Safer>,
    /// What stops it, which ZFS would refuse it for: the plan fails.
    pub stops: Vec<Stop>,
    /// Who else may use the pool it imports.
    pub pool_use: Option<PoolUse>,
}

/// Something a change loses.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Loss {
    /// A snapshot is destroyed.
    Snapshot {
        /// The snapshot.
        name: SnapshotName,
        /// The space only it holds, in bytes; `None` when unknown.
        used: Option<u64>,
    },
    /// A bookmark is destroyed.
    Bookmark {
        /// The filesystem it is of.
        dataset: DatasetName,
        /// Its own name, after `#`.
        name: String,
    },
    /// What was written to a filesystem since its snapshot `since`.
    Written {
        /// The snapshot.
        since: SnapshotName,
        /// How much, in bytes (`written@<snapshot>`); `None` when unknown.
        bytes: Option<u64>,
    },
    /// Every snapshot and bookmark of the filesystem of `since` that ZFS
    /// took after it, which could not be listed.
    Newer(SnapshotName),
    /// What was written to a pool since its checkpoint, which a rewind
    /// discards: every write, and among them the filesystems and
    /// snapshots made since.
    SinceCheckpoint {
        /// When the checkpoint was taken, as `zpool` prints it; `None`
        /// when unknown.
        created: Option<Untrusted<String>>,
        /// The space the checkpoint holds, in bytes; `None` when unknown.
        space: Option<u64>,
        /// Whether the checkpoint is being discarded.
        discarding: bool,
        /// The filesystems made since; `None` when unknown, as when the
        /// checkpoint's time is.
        datasets: Option<Vec<DatasetName>>,
        /// The snapshots taken since; `None` when unknown.
        snapshots: Option<Vec<SnapshotName>>,
    },
}

/// Something a change keeps.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Keep {
    /// This many snapshots older than those it loses.
    Snapshots(usize),
    /// This many bookmarks: those older than the ones a rollback loses,
    /// and every one of the filesystem whose snapshots a destroy loses.
    Bookmarks(usize),
}

/// An action that gives most of what a change gives, and loses nothing.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Safer {
    /// Cloning the snapshot as a new filesystem, which the menu names,
    /// instead of rolling back to it.
    Clone(SnapshotName),
    /// This import, which writes nothing.
    Import(Box<Action>),
}

/// What makes ZFS refuse a change.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Stop {
    /// A snapshot the change destroys has a clone.
    Clone {
        /// The snapshot.
        snapshot: SnapshotName,
        /// The filesystem cloned from it.
        clone: Untrusted<String>,
    },
    /// A snapshot the change destroys has a hold.
    Hold {
        /// The snapshot.
        snapshot: SnapshotName,
        /// The hold's tag; `None` when the holds could not be read.
        tag: Option<Untrusted<String>>,
    },
    /// The pool is imported by another system now, which `zpool import
    /// -f` refuses to take it from.
    Active {
        /// The pool.
        pool: PoolName,
        /// That system, as the pool records it; `None` when unknown.
        host: Option<PoolHost>,
    },
}

/// Who else may use a pool that a change imports.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PoolUse {
    /// The host that last imported it, as the pool records it; `None`
    /// when unknown, or when it is this host.
    pub host: Option<PoolHost>,
    /// When the pool was last written, in seconds since 1970; `None` when
    /// unknown.
    pub written: Option<u64>,
    /// Whether this host has it imported: no other uses it now.
    pub this_host: bool,
    /// Whether another system has it imported now.
    pub active: bool,
}
