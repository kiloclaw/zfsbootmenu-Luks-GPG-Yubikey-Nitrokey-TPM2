//! What a front end's [`App`](super::App) keeps between two reads of the
//! machine ([`Seen`]): what costs too much to read in each snapshot.

use std::sync::{Mutex, PoisonError};

use zbm_types::DatasetName;

use crate::model::Pool;

/// What the application saw that the snapshot does not read again: the
/// pools the last scan ([`Action::ScanPools`](crate::model::Action)) found
/// that could be imported, as `zpool import` looks at every device; and
/// the column of snapshots open, the environment whose snapshots were
/// loaded last ([`Action::LoadSnapshots`](crate::model::Action)), which
/// each snapshot reads again. A front end owns one for as long as its menu
/// runs, and lends it to its application
/// ([`App::with_seen`](super::App::with_seen)); the scan and the loads
/// fill it, and every snapshot lists what it holds.
#[derive(Debug, Default)]
pub struct Seen {
    importable: Mutex<Vec<Pool>>,
    column: Mutex<Option<DatasetName>>,
}

impl Seen {
    /// Nothing seen yet: no pool to import until the first scan, and no
    /// column open.
    pub fn new() -> Seen {
        Seen::default()
    }

    /// The pools that could be imported when the last scan ran.
    pub(crate) fn importable(&self) -> Vec<Pool> {
        self.importable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Keeps `pools`, which a scan found, in place of those of the last.
    pub(crate) fn keep(&self, pools: Vec<Pool>) {
        *self
            .importable
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = pools;
    }

    /// The environment whose snapshots were loaded last, if any.
    pub(crate) fn column(&self) -> Option<DatasetName> {
        self.column
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Opens the column of `environment`, in place of the last; `None`
    /// closes it.
    pub(crate) fn open_column(&self, environment: Option<DatasetName>) {
        *self.column.lock().unwrap_or_else(PoisonError::into_inner) = environment;
    }
}
