//! The snapshots of the demo machine, kept apart from its [`Snapshot`], as
//! `zbm-app` keeps them on the pools: the snapshot holds each
//! environment's count and newest, and every snapshot of the column open,
//! the environment whose snapshots were loaded last.

use std::collections::BTreeMap;

use zbm_types::DatasetName;

use crate::menu::world::{Event, Outcome, Snapshot, SnapshotInfo, SnapshotList};

/// The snapshots of every environment of the demo machine, newest first,
/// and the column open.
#[derive(Clone, Debug, Default)]
pub(super) struct Lists {
    /// The snapshots of each environment that has any.
    lists: BTreeMap<DatasetName, Vec<SnapshotInfo>>,
    /// The environment whose snapshots were loaded last.
    column: Option<DatasetName>,
}

impl Lists {
    /// The lists `lists`, no column open.
    pub(super) fn new(lists: Vec<SnapshotList>) -> Lists {
        Lists {
            lists: lists
                .into_iter()
                .map(|list| (list.environment, list.snapshots))
                .collect(),
            column: None,
        }
    }

    /// The snapshots of `environment`, newest first; none when it has
    /// none.
    pub(super) fn of(&self, environment: &DatasetName) -> &[SnapshotInfo] {
        self.lists
            .get(environment)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// The snapshots of `environment`, to change, newest first.
    pub(super) fn of_mut(&mut self, environment: &DatasetName) -> &mut Vec<SnapshotInfo> {
        self.lists.entry(environment.clone()).or_default()
    }

    /// Puts into `snapshot` what it holds of the snapshots: the count and
    /// the newest of each environment, and the list of the column open,
    /// while its environment is there.
    pub(super) fn show(&self, snapshot: &mut Snapshot) {
        for environment in &mut snapshot.environments {
            let own = self.of(&environment.name);
            environment.snapshot_count = own.len();
            environment.newest_snapshot = own.iter().max_by_key(|s| s.created).cloned();
        }
        snapshot.loaded = self
            .column
            .as_ref()
            .filter(|column| snapshot.environments.iter().any(|e| e.name == **column))
            .map(|column| SnapshotList {
                environment: column.clone(),
                snapshots: self.of(column).to_vec(),
            })
            .into_iter()
            .collect();
    }

    /// [`Action::LoadSnapshots`](crate::menu::world::Action): the
    /// snapshots of `environment`, whose column the snapshots of the
    /// machine carry from now on; as `zfs` does, a load fails for an
    /// environment the machine no longer has, as after its pool's export.
    pub(super) fn load(&mut self, environment: DatasetName, snapshot: &mut Snapshot) -> Vec<Event> {
        if !snapshot.environments.iter().any(|e| e.name == environment) {
            self.column = None;
            let failed = Outcome::Failed {
                what: format!("unable to read the snapshots of {environment}"),
                why: vec![format!("{environment} does not exist")],
                next: Vec::new(),
            };
            return vec![Event::SnapshotsUnread(environment), Event::Finished(failed)];
        }
        let list = SnapshotList {
            snapshots: self.of(&environment).to_vec(),
            environment: environment.clone(),
        };
        self.column = Some(environment);
        self.show(snapshot);
        vec![Event::SnapshotsLoaded(list)]
    }
}
