//! What an action would do, step by step, with the command each step runs:
//! the menu's dry run (ROADMAP.md, PM8, UI14). [`Action::Plan`] asks for
//! it; [`Event::Planned`] brings it.
//!
//! [`Action::Plan`]: super::Action::Plan
//! [`Event::Planned`]: super::Event::Planned

use zbm_types::{DatasetName, PoolName, SnapshotName};

use super::Outcome;

/// What a step does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StepKind {
    /// The pool, imported read-only, is imported read/write, which may ask
    /// about a hibernated system.
    ImportReadWrite(PoolName),
    /// The key of the encryption root is loaded, which may ask for it.
    LoadKey(DatasetName),
    /// The pool's default boot environment changes.
    SetDefault {
        /// The pool.
        pool: PoolName,
        /// The environment.
        environment: String,
    },
    /// A property of a pool other than its default changes.
    SetPoolProperty {
        /// The pool.
        pool: PoolName,
        /// The property.
        property: String,
    },
    /// A property of a filesystem changes.
    SetProperty {
        /// The filesystem.
        dataset: DatasetName,
        /// The property.
        property: String,
    },
    /// A filesystem's property goes back to its parent's.
    InheritProperty {
        /// The filesystem.
        dataset: DatasetName,
        /// The property.
        property: String,
    },
    /// A snapshot is taken.
    Snapshot(SnapshotName),
    /// A filesystem rolls back to a snapshot; the newer ones are destroyed.
    Rollback(SnapshotName),
    /// A new filesystem is cloned from a snapshot.
    Clone {
        /// The snapshot.
        snapshot: SnapshotName,
        /// The new filesystem.
        target: DatasetName,
    },
    /// A clone is promoted: it no longer depends on its origin.
    Promote(DatasetName),
    /// A snapshot is copied into a new, independent filesystem.
    Copy {
        /// The snapshot.
        snapshot: SnapshotName,
        /// The new filesystem.
        target: DatasetName,
    },
    /// A filesystem takes its parent's key.
    InheritKey(DatasetName),
    /// A pool is exported.
    Export(PoolName),
}

/// A step of a plan: what it does, and the command it runs, when one
/// command does it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Step {
    /// What it does.
    pub kind: StepKind,
    /// The command, as it would run: `zfs snapshot tank/ROOT/be@new`.
    pub command: Option<String>,
}

/// What an action would do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    /// Its steps, in order.
    pub steps: Vec<Step>,
    /// How it would end, as far as can be told without doing it: a step
    /// it would refuse, such as a rollback to a snapshot with no kernel,
    /// fails the plan.
    pub outcome: Outcome,
}
