//! The changes ZFS makes, as data ([`ZfsChange`]), and a [`Zfs`] port that
//! plans them instead of making them ([`DryRun`]): the dry run of an action
//! is the action itself, run against it (ROADMAP.md, PM8). [`ZfsCli`]
//! builds its commands from the same changes, so a plan shows the command
//! that would run.
//!
//! [`ZfsCli`]: crate::ZfsCli

use std::fmt;

use zbm_platform::shell::word;
use zbm_types::{DatasetName, PoolName, SnapshotName};

use crate::model::{
    Checkpoint, Dataset, Difference, Hold, Importable, Pool, PoolHost, PoolStatus, Property,
    Snapshot, SnapshotDetail, SnapshotSet, SnapshotSpace, Version,
};
use crate::port::{Zfs, ZfsError};

/// A change ZFS makes, with what it is made to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ZfsChange {
    /// `zpool export`.
    Export(PoolName),
    /// `zfs set property=value`.
    Set {
        /// The dataset.
        dataset: DatasetName,
        /// The property.
        property: String,
        /// Its new value.
        value: String,
    },
    /// `zfs inherit`.
    Inherit {
        /// The dataset.
        dataset: DatasetName,
        /// The property.
        property: String,
    },
    /// `zpool set property=value`.
    SetPool {
        /// The pool.
        pool: PoolName,
        /// The property.
        property: String,
        /// Its new value.
        value: String,
    },
    /// `zfs snapshot`.
    Snapshot(SnapshotName),
    /// `zfs rollback -r`, which destroys the newer snapshots.
    Rollback(SnapshotName),
    /// `zfs destroy <dataset>@a,b,c`: snapshots of one dataset; none when
    /// one has clones or holds, and those that exist when some do not.
    Destroy(Vec<SnapshotName>),
    /// `zfs clone`, with properties.
    Clone {
        /// The snapshot.
        snapshot: SnapshotName,
        /// The new filesystem.
        target: DatasetName,
        /// Its properties.
        properties: Vec<(String, String)>,
    },
    /// `zfs promote`.
    Promote(DatasetName),
    /// A raw send of a snapshot into a receive, unmounted, with properties.
    SendReceive {
        /// The snapshot.
        snapshot: SnapshotName,
        /// The new filesystem.
        target: DatasetName,
        /// Its properties.
        properties: Vec<(String, String)>,
    },
    /// `zfs change-key -i`: the dataset inherits its parent's key.
    InheritKey(DatasetName),
}

/// `-o name=value` for each of `properties`.
fn property_options(properties: &[(String, String)]) -> Vec<String> {
    properties
        .iter()
        .flat_map(|(name, value)| ["-o".to_owned(), format!("{name}={value}")])
        .collect()
}

/// The arguments of `zfs destroy` that name `snapshots`: one,
/// `<dataset>@a,b,c`, when they are of one dataset, which is what `zfs`
/// takes; one for each dataset otherwise, and none for no snapshot, which
/// `zfs` refuses. A snapshot's name holds no `,`, `%` or `@`, so the list
/// names exactly them.
fn destroy_targets(snapshots: &[SnapshotName]) -> Vec<String> {
    let mut targets: Vec<(&DatasetName, Vec<&str>)> = Vec::new();
    for snapshot in snapshots {
        match targets
            .iter_mut()
            .find(|(dataset, _)| *dataset == snapshot.dataset())
        {
            Some((_, names)) => names.push(snapshot.name()),
            None => targets.push((snapshot.dataset(), vec![snapshot.name()])),
        }
    }
    targets
        .into_iter()
        .map(|(dataset, names)| format!("{dataset}@{}", names.join(",")))
        .collect()
}

impl SnapshotSet {
    /// The arguments of `zfs destroy` that name the set: a range is
    /// `<dataset>@first%last`, or its two ends when they are of two
    /// datasets, which `zfs` refuses.
    pub(crate) fn targets(&self) -> Vec<String> {
        match self {
            SnapshotSet::Each(snapshots) => destroy_targets(snapshots),
            SnapshotSet::Range { first, last } if first.dataset() == last.dataset() => {
                vec![format!("{first}%{}", last.name())]
            }
            SnapshotSet::Range { first, last } => vec![first.to_string(), last.to_string()],
        }
    }
}

impl ZfsChange {
    /// The program and the arguments that make the change, but for
    /// [`ZfsChange::SendReceive`], which is a pipeline: the arguments of
    /// its receive.
    pub fn command(&self) -> (&'static str, Vec<String>) {
        let words = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect();
        match self {
            ZfsChange::Export(pool) => ("zpool", words(&["export", pool.as_str()])),
            ZfsChange::Set {
                dataset,
                property,
                value,
            } => {
                let assignment = format!("{property}={value}");
                ("zfs", words(&["set", &assignment, dataset.as_str()]))
            }
            ZfsChange::Inherit { dataset, property } => {
                ("zfs", words(&["inherit", property, dataset.as_str()]))
            }
            ZfsChange::SetPool {
                pool,
                property,
                value,
            } => {
                let assignment = format!("{property}={value}");
                ("zpool", words(&["set", &assignment, pool.as_str()]))
            }
            ZfsChange::Snapshot(snapshot) => ("zfs", words(&["snapshot", &snapshot.to_string()])),
            ZfsChange::Rollback(snapshot) => {
                ("zfs", words(&["rollback", "-r", &snapshot.to_string()]))
            }
            ZfsChange::Destroy(snapshots) => {
                let mut args = words(&["destroy"]);
                args.extend(destroy_targets(snapshots));
                ("zfs", args)
            }
            ZfsChange::Clone {
                snapshot,
                target,
                properties,
            } => {
                let mut args = words(&["clone"]);
                args.extend(property_options(properties));
                args.extend([snapshot.to_string(), target.to_string()]);
                ("zfs", args)
            }
            ZfsChange::Promote(dataset) => ("zfs", words(&["promote", dataset.as_str()])),
            ZfsChange::SendReceive {
                target, properties, ..
            } => {
                let mut args = words(&["-u"]);
                args.extend(property_options(properties));
                args.push(target.to_string());
                ("zfs", args)
            }
            ZfsChange::InheritKey(dataset) => {
                ("zfs", words(&["change-key", "-i", dataset.as_str()]))
            }
        }
    }
}

impl fmt::Display for ZfsChange {
    /// The command line that makes it, as a shell would take it: `zfs
    /// snapshot tank/ROOT/be@new`, and each argument a shell would split
    /// or read in single quotes, as `zfs set
    /// org.zfsbootmenu:commandline='quiet loglevel=4' tank/ROOT/be`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (program, args) = self.command();
        if let ZfsChange::SendReceive { snapshot, .. } = self {
            let snapshot = word(&snapshot.to_string());
            write!(formatter, "zfs send -p -w {snapshot} | zfs recv")?;
        } else {
            write!(formatter, "{program}")?;
        }
        for arg in args {
            write!(formatter, " {}", word(&arg))?;
        }
        Ok(())
    }
}

/// A [`Zfs`] port that answers what is read from `real`, and plans the
/// changes instead of making them: each goes to `plan`, and succeeds.
pub struct DryRun<'a> {
    real: &'a dyn Zfs,
    plan: &'a dyn Fn(ZfsChange),
}

impl<'a> DryRun<'a> {
    /// The port that reads from `real` and gives the changes to `plan`.
    pub fn new(real: &'a dyn Zfs, plan: &'a dyn Fn(ZfsChange)) -> DryRun<'a> {
        DryRun { real, plan }
    }

    /// Plans `change`, which succeeds, as a change of the port would.
    #[allow(
        clippy::unnecessary_wraps,
        reason = "what the port's changes return, which every one of them passes on"
    )]
    fn planned(&self, change: ZfsChange) -> Result<(), ZfsError> {
        (self.plan)(change);
        Ok(())
    }
}

impl Zfs for DryRun<'_> {
    fn version(&self) -> Result<Version, ZfsError> {
        self.real.version()
    }

    fn pools(&self) -> Result<Vec<Pool>, ZfsError> {
        self.real.pools()
    }

    fn pool_names(&self) -> Result<Vec<PoolName>, ZfsError> {
        self.real.pool_names()
    }

    fn devices(&self, pool: &PoolName) -> Result<Vec<String>, ZfsError> {
        self.real.devices(pool)
    }

    fn status(&self) -> Result<Vec<PoolStatus>, ZfsError> {
        self.real.status()
    }

    fn importable(&self) -> Result<Vec<Importable>, ZfsError> {
        self.real.importable()
    }

    fn pool_host(&self, id: u64) -> Result<PoolHost, ZfsError> {
        self.real.pool_host(id)
    }

    fn checkpoint(&self, pool: &PoolName) -> Result<Option<Checkpoint>, ZfsError> {
        self.real.checkpoint(pool)
    }

    fn export(&self, pool: &PoolName) -> Result<(), ZfsError> {
        self.planned(ZfsChange::Export(pool.clone()))
    }

    fn pool_property(&self, pool: &PoolName, property: &str) -> Result<Property, ZfsError> {
        self.real.pool_property(pool, property)
    }

    fn filesystems(
        &self,
        root: Option<&DatasetName>,
        properties: &[&str],
    ) -> Result<Vec<Dataset>, ZfsError> {
        self.real.filesystems(root, properties)
    }

    fn snapshots(&self, dataset: &DatasetName) -> Result<Vec<Snapshot>, ZfsError> {
        self.real.snapshots(dataset)
    }

    fn own_snapshots(&self, dataset: &DatasetName) -> Result<Vec<SnapshotSpace>, ZfsError> {
        self.real.own_snapshots(dataset)
    }

    fn snapshot_details(&self, dataset: &DatasetName) -> Result<Vec<SnapshotDetail>, ZfsError> {
        self.real.snapshot_details(dataset)
    }

    fn holds(&self, snapshots: &[SnapshotName]) -> Result<Vec<Hold>, ZfsError> {
        self.real.holds(snapshots)
    }

    /// A read: `zfs destroy -n` destroys nothing.
    fn destroy_space(&self, snapshots: &SnapshotSet) -> Result<u64, ZfsError> {
        self.real.destroy_space(snapshots)
    }

    fn property(&self, dataset: &DatasetName, property: &str) -> Result<Property, ZfsError> {
        self.real.property(dataset, property)
    }

    fn set(&self, dataset: &DatasetName, property: &str, value: &str) -> Result<(), ZfsError> {
        self.planned(ZfsChange::Set {
            dataset: dataset.clone(),
            property: property.to_owned(),
            value: value.to_owned(),
        })
    }

    fn inherit(&self, dataset: &DatasetName, property: &str) -> Result<(), ZfsError> {
        self.planned(ZfsChange::Inherit {
            dataset: dataset.clone(),
            property: property.to_owned(),
        })
    }

    fn set_pool_property(
        &self,
        pool: &PoolName,
        property: &str,
        value: &str,
    ) -> Result<(), ZfsError> {
        self.planned(ZfsChange::SetPool {
            pool: pool.clone(),
            property: property.to_owned(),
            value: value.to_owned(),
        })
    }

    fn snapshot_property(
        &self,
        snapshot: &SnapshotName,
        property: &str,
    ) -> Result<Property, ZfsError> {
        self.real.snapshot_property(snapshot, property)
    }

    fn own_properties(&self, dataset: &DatasetName) -> Result<Vec<(String, Property)>, ZfsError> {
        self.real.own_properties(dataset)
    }

    fn create_snapshot(&self, snapshot: &SnapshotName) -> Result<(), ZfsError> {
        self.planned(ZfsChange::Snapshot(snapshot.clone()))
    }

    fn rollback(&self, snapshot: &SnapshotName) -> Result<(), ZfsError> {
        self.planned(ZfsChange::Rollback(snapshot.clone()))
    }

    fn destroy_snapshots(&self, snapshots: &[SnapshotName]) -> Result<(), ZfsError> {
        self.planned(ZfsChange::Destroy(snapshots.to_vec()))
    }

    fn clone_snapshot(
        &self,
        snapshot: &SnapshotName,
        target: &DatasetName,
        properties: &[(String, String)],
    ) -> Result<(), ZfsError> {
        self.planned(ZfsChange::Clone {
            snapshot: snapshot.clone(),
            target: target.clone(),
            properties: properties.to_vec(),
        })
    }

    fn promote(&self, dataset: &DatasetName) -> Result<(), ZfsError> {
        self.planned(ZfsChange::Promote(dataset.clone()))
    }

    fn send_receive(
        &self,
        snapshot: &SnapshotName,
        target: &DatasetName,
        properties: &[(String, String)],
    ) -> Result<(), ZfsError> {
        self.planned(ZfsChange::SendReceive {
            snapshot: snapshot.clone(),
            target: target.clone(),
            properties: properties.to_vec(),
        })
    }

    fn inherit_key(&self, dataset: &DatasetName) -> Result<(), ZfsError> {
        self.planned(ZfsChange::InheritKey(dataset.clone()))
    }

    fn diff(
        &self,
        from: &SnapshotName,
        to: Option<&SnapshotName>,
    ) -> Result<Vec<Difference>, ZfsError> {
        self.real.diff(from, to)
    }
}

#[cfg(test)]
mod tests;
