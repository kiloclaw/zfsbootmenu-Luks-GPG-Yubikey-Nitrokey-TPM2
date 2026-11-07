//! Destroying snapshots of one boot environment: all of them in one
//! `zfs destroy <environment>@a,b,c`, which destroys none when one has a
//! clone or a hold. Those are named first ([`stops`]), before the pool is
//! imported read/write, and so is a filesystem that is not a boot
//! environment, whose snapshots the menu never shows (ADR 0016, point 14).
//! The environment's bookmarks stay.

use zbm_types::{DatasetName, SnapshotName};

use super::{Changes, Failure, done, stops};
use crate::model::Event;
use crate::read;

impl Changes<'_, '_> {
    /// Destroys `snapshots`, each once, which must be of one boot
    /// environment.
    pub(super) fn destroy(&mut self, snapshots: &[SnapshotName]) -> Result<Vec<Event>, Failure> {
        let mut named: Vec<SnapshotName> = Vec::with_capacity(snapshots.len());
        for snapshot in snapshots {
            if !named.contains(snapshot) {
                named.push(snapshot.clone());
            }
        }
        let Some(environment) = named.first().map(|first| first.dataset().clone()) else {
            return Err(Failure::new(
                "unable to destroy snapshots",
                vec!["no snapshot was chosen".to_owned()],
            ));
        };
        let what = format!("destroy {} of {environment}", counted(named.len()));
        let unable = |why: String| Failure::new(format!("unable to {what}"), vec![why]);
        if let Some(other) = named.iter().find(|name| *name.dataset() != environment) {
            return Err(unable(format!("{other} is not of {environment}")));
        }
        if !read::is_environment(self.zfs, &environment) {
            return Err(unable(format!("{environment} is not a boot environment")));
        }
        let stops = stops::of_destroy(self.zfs, &named);
        if !stops.is_empty() {
            let why = stops.iter().map(stops::why).collect();
            return Err(Failure::new(format!("unable to {what}"), why));
        }
        self.writable(&environment.pool(), &what)?;
        self.zfs
            .destroy_snapshots(&named)
            .map_err(|error| Failure::zfs(format!("unable to {what}"), &error))?;
        Ok(done(destroyed(&environment, &named)))
    }
}

/// `count` snapshots, in words: `a snapshot`, `3 snapshots`.
fn counted(count: usize) -> String {
    if count == 1 {
        "a snapshot".to_owned()
    } else {
        format!("{count} snapshots")
    }
}

/// What a destroy of `snapshots` of `environment` says it did.
fn destroyed(environment: &DatasetName, snapshots: &[SnapshotName]) -> String {
    let names: Vec<&str> = snapshots.iter().map(SnapshotName::name).collect();
    format!("{environment}@{} destroyed", names.join(","))
}
