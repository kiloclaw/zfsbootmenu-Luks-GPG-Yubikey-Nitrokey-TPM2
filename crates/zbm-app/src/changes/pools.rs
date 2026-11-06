//! Exporting a pool from the menu ([`Action::Export`]): `zpool export`
//! through the ZFS port, whose errors come back as the failure's causes,
//! as a pool that is busy gives; nothing is written on the console. The
//! menu holds no mount between two actions, so only what another program
//! keeps open makes the pool busy. The volumes the pool is on stay open.
//!
//! [`Action::Export`]: crate::model::Action::Export

use zbm_types::PoolName;

use super::{Changes, Failure, done};
use crate::model::Event;

impl Changes<'_, '_> {
    /// Exports `pool`.
    pub(super) fn export(&mut self, pool: &PoolName) -> Result<Vec<Event>, Failure> {
        self.zfs
            .export(pool)
            .map_err(|error| Failure::zfs(format!("unable to export {pool}"), &error))?;
        Ok(done(format!("{pool} is exported")))
    }
}
