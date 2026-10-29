//! Package D: the changes the menu makes to boot environments and their
//! snapshots (ROADMAP.md, R5 step 5). Each change has its pool
//! imported read/write and the key of its environment loaded first, as
//! the legacy functions do, then runs the commands of the legacy runtime
//! through the ZFS port:
//!
//! - a new default: `zpool set bootfs=`;
//! - a snapshot: `zfs snapshot`;
//! - a rollback, to a snapshot that has kernels: `zfs rollback -r`;
//! - a clone, promoted or not: `zfs clone` with the environment's own
//!   properties, then `zfs promote`;
//! - a duplicate: a raw send into a receive, when the space is there, then
//!   the copy inherits its parent's key where the source did;
//! - the differences of a snapshot: `zfs diff` on the mounted filesystem.
//!
//! What a change needs besides ZFS ([`Needs`]) comes from the console when
//! the change may ask for something ([`Action::needs_console_on`]), and
//! without it in the menu otherwise.

mod needs;

pub(crate) use needs::{Needs, OnMachine, Planning};

use zbm_types::Untrusted;
use zbm_types::{DatasetName, PoolName, SnapshotName};
use zbm_zfs::{DiffChange, Difference, Zfs, ZfsError};

use crate::model::{Action, Change, ChangeKind, Event, Outcome};

/// The properties a clone does not take from its environment: those it
/// is given ([`clone_options`]), and the encryption ones, which only an
/// encryption root has and a clone cannot set.
const NOT_CLONED: &[&str] = &[
    "mountpoint",
    "canmount",
    "encryption",
    "keylocation",
    "keyformat",
    "pbkdf2iters",
];

/// Why a change failed: what failed, and why.
struct Failure {
    what: String,
    why: Vec<String>,
}

impl Failure {
    fn new(what: impl Into<String>, why: Vec<String>) -> Failure {
        Failure {
            what: what.into(),
            why,
        }
    }

    /// The failure of `what` because ZFS failed.
    fn zfs(what: impl Into<String>, error: &ZfsError) -> Failure {
        Failure::new(what, vec![error.to_string()])
    }

    fn into_event(self) -> Event {
        Event::Finished(Outcome::Failed {
            what: self.what,
            why: self.why,
            next: Vec::new(),
        })
    }
}

/// What a change works with.
pub(crate) struct Changes<'z, 'n> {
    /// The ZFS port.
    pub zfs: &'z dyn Zfs,
    /// The read/write import, keys and mounts.
    pub needs: &'n mut dyn Needs,
}

impl Changes<'_, '_> {
    /// Performs `action` if it is a change; `None` for other actions.
    pub fn perform(&mut self, action: &Action) -> Option<Vec<Event>> {
        let result = match action {
            Action::SetDefaultEnvironment(environment) => self.set_default(environment),
            Action::CreateSnapshot(snapshot) => self.create_snapshot(snapshot),
            Action::Rollback(snapshot) => self.rollback(snapshot),
            Action::Clone(snapshot, target) => self.clone_snapshot(snapshot, target, false),
            Action::CloneAndPromote(snapshot, target) => {
                self.clone_snapshot(snapshot, target, true)
            }
            Action::Duplicate(snapshot, target) => self.duplicate(snapshot, target),
            Action::Diff(from, to) => self.diff(from, to.as_ref()),
            _ => return None,
        };
        Some(result.unwrap_or_else(|failure| vec![failure.into_event()]))
    }

    /// Imports `pool` read/write unless it is, for `what`.
    fn writable(&mut self, pool: &PoolName, what: &str) -> Result<(), Failure> {
        self.needs
            .writable(pool)
            .map_err(|why| Failure::new(format!("unable to {what}"), why))
    }

    /// Loads the key of `dataset` unless it is loaded, for `what`.
    fn load_key(&mut self, dataset: &DatasetName, what: &str) -> Result<(), Failure> {
        self.needs
            .load_key(dataset)
            .map_err(|why| Failure::new(format!("unable to {what}"), why))
    }

    fn set_default(&mut self, environment: &DatasetName) -> Result<Vec<Event>, Failure> {
        let what = format!("make {environment} the default");
        let pool = environment.pool();
        self.writable(&pool, &what)?;
        self.zfs
            .set_pool_property(&pool, "bootfs", environment.as_str())
            .map_err(|error| Failure::zfs(format!("unable to {what}"), &error))?;
        Ok(done(format!("{environment} is the default of {pool}")))
    }

    fn create_snapshot(&mut self, snapshot: &SnapshotName) -> Result<Vec<Event>, Failure> {
        let what = format!("take {snapshot}");
        self.writable(&snapshot.pool(), &what)?;
        self.load_key(snapshot.dataset(), &what)?;
        self.zfs
            .create_snapshot(snapshot)
            .map_err(|error| Failure::zfs(format!("unable to {what}"), &error))?;
        Ok(done(format!("{snapshot} taken")))
    }

    fn rollback(&mut self, snapshot: &SnapshotName) -> Result<Vec<Event>, Failure> {
        let what = format!("roll {} back to {}", snapshot.dataset(), snapshot.name());
        if !self.needs.has_kernels(snapshot) {
            return Err(Failure::new(
                format!("{snapshot} has no kernels, will not roll back"),
                vec!["use a recovery shell to roll back by hand".to_owned()],
            ));
        }
        self.writable(&snapshot.pool(), &what)?;
        self.load_key(snapshot.dataset(), &what)?;
        self.zfs
            .rollback(snapshot)
            .map_err(|error| Failure::zfs(format!("unable to {what}"), &error))?;
        Ok(done(format!(
            "{} rolled back to {}; the newer snapshots are destroyed",
            snapshot.dataset(),
            snapshot.name()
        )))
    }

    /// The properties of a clone of `parent`: mounted at `/`, not
    /// automatically, and the properties set on `parent` itself.
    fn clone_options(&self, parent: &DatasetName) -> Result<Vec<(String, String)>, ZfsError> {
        let mut options = given();
        options.extend(
            self.zfs
                .own_properties(parent)?
                .into_iter()
                .filter(|(name, _)| !NOT_CLONED.contains(&name.as_str()))
                .map(|(name, property)| (name, property.value)),
        );
        Ok(options)
    }

    fn clone_snapshot(
        &mut self,
        snapshot: &SnapshotName,
        target: &DatasetName,
        promote: bool,
    ) -> Result<Vec<Event>, Failure> {
        let what = format!("create {target} from {snapshot}");
        same_pool(snapshot, target, &what)?;
        self.writable(&snapshot.pool(), &what)?;
        let parent = snapshot.dataset();
        self.load_key(parent, &what)?;
        let options = self
            .clone_options(parent)
            .map_err(|error| Failure::zfs(format!("unable to {what}"), &error))?;
        self.zfs
            .clone_snapshot(snapshot, target, &options)
            .map_err(|error| Failure::zfs(format!("unable to {what}"), &error))?;
        if !promote {
            return Ok(done(format!("{target} cloned from {snapshot}")));
        }
        self.zfs
            .promote(target)
            .map_err(|error| Failure::zfs(format!("unable to promote {target}"), &error))?;
        Ok(done(format!(
            "{target} cloned from {snapshot} and promoted: {parent} depends on it now"
        )))
    }

    fn duplicate(
        &mut self,
        snapshot: &SnapshotName,
        target: &DatasetName,
    ) -> Result<Vec<Event>, Failure> {
        let what = format!("create {target} from {snapshot}");
        same_pool(snapshot, target, &what)?;
        self.writable(&snapshot.pool(), &what)?;
        let source = snapshot.dataset();
        self.load_key(source, &what)?;
        let parent = parent_of(target);
        if let Some(parent) = &parent {
            self.load_key(parent, &what)?;
            self.has_room(snapshot, parent)?;
        }
        let root = self
            .zfs
            .property(source, "encryptionroot")
            .ok()
            .and_then(|property| property.value().map(str::to_owned));
        let mut options = given();
        if let Some(location) = root.as_deref().and_then(|root| self.keylocation(root)) {
            options.push(("keylocation".to_owned(), location));
        }
        self.zfs
            .send_receive(snapshot, target, &options)
            .map_err(|error| Failure::zfs(format!("unable to {what}"), &error))?;
        // A raw copy of an encrypted snapshot is its own encryption root;
        // where the source was not, the copy takes its parent's key.
        if root.is_some_and(|root| root != source.as_str()) && self.is_own_root(target) {
            self.load_key(target, &what)?;
            self.zfs.inherit_key(target).map_err(|error| {
                Failure::zfs(format!("unable to give {target} its parent's key"), &error)
            })?;
        }
        Ok(done(format!("{target} copied from {snapshot}")))
    }

    /// The `keylocation` of the encryption root `root`, unless unset.
    fn keylocation(&self, root: &str) -> Option<String> {
        let root: DatasetName = root.parse().ok()?;
        let property = self.zfs.property(&root, "keylocation").ok()?;
        property.value().map(str::to_owned)
    }

    /// Whether `dataset` is its own encryption root.
    fn is_own_root(&self, dataset: &DatasetName) -> bool {
        self.zfs
            .property(dataset, "encryptionroot")
            .is_ok_and(|property| property.value == dataset.as_str())
    }

    /// Fails unless `parent` has more space available than `snapshot`
    /// refers to; passes when either cannot be read.
    fn has_room(&self, snapshot: &SnapshotName, parent: &DatasetName) -> Result<(), Failure> {
        let available = number(self.zfs.property(parent, "available"));
        let needed = number(self.zfs.snapshot_property(snapshot, "referenced"));
        match (available, needed) {
            (Some(available), Some(needed)) if available <= needed => Err(Failure::new(
                "insufficient space for duplication",
                vec![format!(
                    "{parent} has {} free but needs {}",
                    bytes(available),
                    bytes(needed)
                )],
            )),
            _ => Ok(()),
        }
    }

    fn diff(
        &mut self,
        first: &SnapshotName,
        second: Option<&SnapshotName>,
    ) -> Result<Vec<Event>, Failure> {
        let (from, to) = match second {
            Some(second) if self.created(second) < self.created(first) => {
                (second.clone(), Some(first.clone()))
            }
            _ => (first.clone(), second.cloned()),
        };
        let what = format!("compare {from}");
        self.writable(&from.pool(), &what)?;
        let filesystem = from.dataset();
        self.load_key(filesystem, &what)?;
        let mounted = self
            .needs
            .mount(filesystem)
            .map_err(|why| Failure::new(format!("unable to mount {filesystem}"), why))?;
        let found = self.zfs.diff(&from, to.as_ref());
        self.needs.unmount(&mounted);
        let found = found.map_err(|error| Failure::zfs(format!("unable to {what}"), &error))?;
        let changes = found
            .into_iter()
            .map(|difference| change(difference, &mounted))
            .collect();
        Ok(vec![Event::DiffReady {
            from,
            to,
            changes,
            packages: None,
        }])
    }

    /// When `snapshot` was taken, 0 when unknown.
    fn created(&self, snapshot: &SnapshotName) -> u64 {
        number(self.zfs.snapshot_property(snapshot, "creation")).unwrap_or_default()
    }
}

/// The properties every new environment is given: mounted at `/`, not
/// automatically.
fn given() -> Vec<(String, String)> {
    vec![
        ("mountpoint".to_owned(), "/".to_owned()),
        ("canmount".to_owned(), "noauto".to_owned()),
    ]
}

/// A done outcome, as the last event.
fn done(what: String) -> Vec<Event> {
    vec![Event::Finished(Outcome::Done(what))]
}

/// Fails unless `target` is in the pool of `snapshot`, as clones and the
/// copies of the menu are.
fn same_pool(snapshot: &SnapshotName, target: &DatasetName, what: &str) -> Result<(), Failure> {
    if target.pool() == snapshot.pool() {
        return Ok(());
    }
    Err(Failure::new(
        format!("unable to {what}"),
        vec![format!("{target} is not in {}", snapshot.pool())],
    ))
}

/// The parent of `dataset`, unless it is the root of its pool.
fn parent_of(dataset: &DatasetName) -> Option<DatasetName> {
    let (parent, _) = dataset.as_str().rsplit_once('/')?;
    parent.parse().ok()
}

/// The number a property holds, as `zfs get -p` prints it.
fn number(property: Result<zbm_zfs::Property, ZfsError>) -> Option<u64> {
    property.ok()?.value.parse().ok()
}

/// `bytes` for people, in binary units: `1.5 GiB`.
fn bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes;
    let mut tenths = 0;
    let mut unit = 0;
    while value >= 1024 && unit + 1 < UNITS.len() {
        tenths = (value % 1024) * 10 / 1024;
        value /= 1024;
        unit += 1;
    }
    let name = UNITS.get(unit).copied().unwrap_or_default();
    if unit == 0 {
        format!("{value} {name}")
    } else {
        format!("{value}.{tenths} {name}")
    }
}

/// A difference as the menu shows it: its path below the filesystem's
/// mount point `mounted`.
fn change(difference: Difference, mounted: &str) -> Change {
    let below = |path: String| {
        Untrusted::new(match path.strip_prefix(mounted) {
            Some("") => "/".to_owned(),
            Some(rest) if rest.starts_with('/') => rest.to_owned(),
            _ => path,
        })
    };
    Change {
        kind: match difference.change {
            DiffChange::Removed => ChangeKind::Removed,
            DiffChange::Created => ChangeKind::Created,
            DiffChange::Modified => ChangeKind::Modified,
            DiffChange::Renamed => ChangeKind::Renamed,
        },
        path: below(difference.path),
        renamed_to: difference.renamed_to.map(below),
        preview: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
