//! `FakeZfs`: an in-memory model of pools, datasets and snapshots that
//! answers the ZFS port (`zbm_zfs::Zfs`), for tests of code that asks ZFS
//! questions without running `zfs`.
//!
//! Tests describe the machine with builders and read back what the code did
//! with [`FakeZfs::calls`]:
//!
//! ```
//! use zbm_testkit::zfs::FakeZfs;
//! use zbm_zfs::Zfs;
//!
//! let zfs = FakeZfs::new()
//!     .pool("zroot")
//!     .filesystem("zroot/ROOT/void", &[("mountpoint", "/")])
//!     .bootfs("zroot", "zroot/ROOT/void");
//! let pools = zfs.pools().unwrap();
//! assert_eq!(pools[0].bootfs.as_ref().unwrap().as_str(), "zroot/ROOT/void");
//! assert_eq!(zfs.calls(), ["pools"]);
//! ```
//!
//! Properties follow ZFS: a value set on a dataset is `local`; a property not
//! set is inherited from the nearest ancestor that sets it; otherwise it has
//! its default, or `-` without a source when it has none.

use std::cell::RefCell;
use std::collections::BTreeMap;

use zbm_types::{DatasetName, PoolName, SnapshotName};
use zbm_zfs::{
    Dataset, Difference, Health, Pool, Property, Snapshot, Source, Version, Zfs, ZfsError,
};

mod changes;

/// The defaults of the native properties the fake knows.
const DEFAULTS: &[(&str, &str)] = &[
    ("canmount", "on"),
    ("encryption", "off"),
    ("encryptionroot", "-"),
    ("keyformat", "none"),
    ("keylocation", "none"),
    ("keystatus", "-"),
    ("readonly", "off"),
];

/// Properties that a child never inherits.
const NOT_INHERITED: &[&str] = &["canmount", "encryptionroot", "keystatus", "keyformat"];

/// A pool of the fake.
#[derive(Clone, Debug)]
struct FakePool {
    health: Health,
    read_only: bool,
    bootfs: Option<DatasetName>,
    guid: u64,
    properties: BTreeMap<String, String>,
    devices: Vec<String>,
    busy: bool,
}

/// The machine the fake describes.
#[derive(Debug, Default)]
struct State {
    pools: BTreeMap<PoolName, FakePool>,
    datasets: BTreeMap<DatasetName, BTreeMap<String, String>>,
    snapshots: BTreeMap<SnapshotName, u64>,
    /// The snapshot each clone was made from.
    origins: BTreeMap<DatasetName, SnapshotName>,
    differences: BTreeMap<(SnapshotName, Option<SnapshotName>), Vec<Difference>>,
    /// The changes planned to fail, by operation, with their messages.
    failures: BTreeMap<String, String>,
    calls: Vec<String>,
}

/// An in-memory ZFS; see the [module documentation](self).
#[derive(Debug)]
pub struct FakeZfs {
    state: RefCell<State>,
    version: Version,
}

impl Default for FakeZfs {
    fn default() -> FakeZfs {
        FakeZfs::new()
    }
}

/// Parses a name in a builder, where a bad name is a mistake in the test.
fn name<T: std::str::FromStr>(text: &str) -> T {
    text.parse()
        .unwrap_or_else(|_| panic!("invalid name {text:?} in a FakeZfs builder"))
}

impl FakeZfs {
    /// No pools, ZFS 2.4.4.
    pub fn new() -> FakeZfs {
        FakeZfs {
            state: RefCell::default(),
            version: Version {
                userland: "zfs-2.4.4-1".to_owned(),
                kernel: "zfs-kmod-2.4.4-1".to_owned(),
            },
        }
    }

    /// The same machine with another version of ZFS.
    #[must_use]
    pub fn release(mut self, userland: &str, kernel: &str) -> FakeZfs {
        self.version = Version {
            userland: userland.to_owned(),
            kernel: kernel.to_owned(),
        };
        self
    }

    /// Adds an online pool `pool` and its root dataset.
    #[must_use]
    pub fn pool(self, pool: &str) -> FakeZfs {
        {
            let mut state = self.state.borrow_mut();
            let guid = 1000 + u64::try_from(state.pools.len()).unwrap_or_default();
            state.pools.insert(
                name(pool),
                FakePool {
                    health: Health::Online,
                    read_only: false,
                    bootfs: None,
                    guid,
                    properties: BTreeMap::new(),
                    devices: Vec::new(),
                    busy: false,
                },
            );
            state.datasets.entry(name(pool)).or_default();
        }
        self
    }

    /// Adds the filesystem `dataset`, its missing parents, and sets
    /// `properties` on it.
    #[must_use]
    pub fn filesystem(self, dataset: &str, properties: &[(&str, &str)]) -> FakeZfs {
        {
            let mut state = self.state.borrow_mut();
            let mut path = String::new();
            for component in dataset.split('/') {
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(component);
                state.datasets.entry(name(&path)).or_default();
            }
            let local = state.datasets.entry(name(dataset)).or_default();
            for (property, value) in properties {
                local.insert((*property).to_owned(), (*value).to_owned());
            }
        }
        self
    }

    /// Adds the snapshot `snapshot`, taken at `creation` seconds.
    #[must_use]
    pub fn snapshot(self, snapshot: &str, creation: u64) -> FakeZfs {
        self.state
            .borrow_mut()
            .snapshots
            .insert(name(snapshot), creation);
        self
    }

    /// Sets the `bootfs` of `pool`.
    #[must_use]
    pub fn bootfs(self, pool: &str, dataset: &str) -> FakeZfs {
        self.with_pool(pool, |fake| fake.bootfs = Some(name(dataset)))
    }

    /// Marks `pool` as imported read-only.
    #[must_use]
    pub fn read_only(self, pool: &str) -> FakeZfs {
        self.with_pool(pool, |fake| fake.read_only = true)
    }

    /// Sets the health of `pool`.
    #[must_use]
    pub fn health(self, pool: &str, health: Health) -> FakeZfs {
        self.with_pool(pool, |fake| fake.health = health)
    }

    /// Sets a property of `pool`, such as `feature@encryption`.
    #[must_use]
    pub fn pool_setting(self, pool: &str, property: &str, value: &str) -> FakeZfs {
        self.with_pool(pool, |fake| {
            fake.properties
                .insert(property.to_owned(), value.to_owned());
        })
    }

    /// Adds `device`, such as `/dev/dm-3`, to the vdevs of `pool`.
    #[must_use]
    pub fn device(self, pool: &str, device: &str) -> FakeZfs {
        self.with_pool(pool, |fake| fake.devices.push(device.to_owned()))
    }

    /// Makes `pool` busy, so that exporting it fails.
    #[must_use]
    pub fn busy(self, pool: &str) -> FakeZfs {
        self.with_pool(pool, |fake| fake.busy = true)
    }

    /// Makes the filesystem `dataset` a clone of `snapshot`.
    #[must_use]
    pub fn origin(self, dataset: &str, snapshot: &str) -> FakeZfs {
        self.state
            .borrow_mut()
            .origins
            .insert(name(dataset), name(snapshot));
        self
    }

    /// What `diff` answers for `from` and `to`; nothing differs otherwise.
    #[must_use]
    pub fn differences(self, from: &str, to: Option<&str>, found: &[Difference]) -> FakeZfs {
        self.state
            .borrow_mut()
            .differences
            .insert((name(from), to.map(name)), found.to_vec());
        self
    }

    /// Makes the change `operation`, such as `rollback`, fail with
    /// `message`.
    #[must_use]
    pub fn failing(self, operation: &str, message: &str) -> FakeZfs {
        self.state
            .borrow_mut()
            .failures
            .insert(operation.to_owned(), message.to_owned());
        self
    }

    /// The snapshot `dataset` was cloned from, if it is a clone.
    pub fn origin_of(&self, dataset: &str) -> Option<String> {
        let state = self.state.borrow();
        state
            .origins
            .get(&name::<DatasetName>(dataset))
            .map(ToString::to_string)
    }

    fn with_pool(self, pool: &str, change: impl FnOnce(&mut FakePool)) -> FakeZfs {
        {
            let mut state = self.state.borrow_mut();
            let fake = state
                .pools
                .get_mut(&name::<PoolName>(pool))
                .unwrap_or_else(|| panic!("FakeZfs has no pool {pool}"));
            change(fake);
        }
        self
    }

    /// Each operation asked of the fake, such as `property zroot/ROOT/void
    /// mountpoint`.
    pub fn calls(&self) -> Vec<String> {
        self.state.borrow().calls.clone()
    }

    fn record(&self, call: String) {
        self.state.borrow_mut().calls.push(call);
    }

    /// The property of a dataset that exists, following inheritance.
    fn resolve(state: &State, dataset: &DatasetName, property: &str) -> Property {
        if let Some(value) = state
            .datasets
            .get(dataset)
            .and_then(|local| local.get(property))
        {
            return Property::new(value.clone(), Source::Local);
        }
        if !NOT_INHERITED.contains(&property) {
            let mut ancestor = dataset.as_str();
            while let Some((parent, _)) = ancestor.rsplit_once('/') {
                ancestor = parent;
                let value = state
                    .datasets
                    .iter()
                    .find(|(candidate, _)| candidate.as_str() == ancestor)
                    .and_then(|(_, local)| local.get(property));
                if let Some(value) = value {
                    return Property::new(value.clone(), Source::Inherited(ancestor.to_owned()));
                }
            }
        }
        match DEFAULTS.iter().find(|(known, _)| *known == property) {
            Some((_, value)) => Property::new(*value, Source::Default),
            None if property == "mountpoint" => {
                Property::new(format!("/{dataset}"), Source::Default)
            }
            None => Property::new("-", Source::None),
        }
    }
}

impl Zfs for FakeZfs {
    fn version(&self) -> Result<Version, ZfsError> {
        self.record("version".to_owned());
        Ok(self.version.clone())
    }

    fn pools(&self) -> Result<Vec<Pool>, ZfsError> {
        self.record("pools".to_owned());
        let state = self.state.borrow();
        Ok(state
            .pools
            .iter()
            .map(|(name, fake)| Pool {
                name: name.clone(),
                health: fake.health.clone(),
                read_only: fake.read_only,
                bootfs: fake.bootfs.clone(),
                guid: fake.guid,
            })
            .collect())
    }

    fn pool_names(&self) -> Result<Vec<PoolName>, ZfsError> {
        self.record("pool_names".to_owned());
        Ok(self.state.borrow().pools.keys().cloned().collect())
    }

    fn devices(&self, pool: &PoolName) -> Result<Vec<String>, ZfsError> {
        self.record(format!("devices {pool}"));
        let state = self.state.borrow();
        let fake = state
            .pools
            .get(pool)
            .ok_or_else(|| ZfsError::Missing(pool.to_string()))?;
        Ok(fake.devices.clone())
    }

    fn export(&self, pool: &PoolName) -> Result<(), ZfsError> {
        self.record(format!("export {pool}"));
        let mut state = self.state.borrow_mut();
        match state.pools.get(pool) {
            None => return Err(ZfsError::Missing(pool.to_string())),
            Some(fake) if fake.busy => {
                return Err(ZfsError::Failed {
                    command: format!("zpool export {pool}"),
                    status: 1,
                    message: format!("cannot export '{pool}': pool is busy"),
                });
            }
            Some(_) => {}
        }
        state.pools.remove(pool);
        let pool = pool.as_str();
        state
            .datasets
            .retain(|dataset, _| dataset.pool().as_str() != pool);
        state
            .snapshots
            .retain(|snapshot, _| snapshot.pool().as_str() != pool);
        Ok(())
    }

    fn pool_property(&self, pool: &PoolName, property: &str) -> Result<Property, ZfsError> {
        self.record(format!("pool_property {pool} {property}"));
        let state = self.state.borrow();
        let fake = state
            .pools
            .get(pool)
            .ok_or_else(|| ZfsError::Missing(pool.to_string()))?;
        let value = match property {
            "readonly" => if fake.read_only { "on" } else { "off" }.to_owned(),
            "bootfs" => fake
                .bootfs
                .as_ref()
                .map_or_else(|| "-".to_owned(), ToString::to_string),
            "health" => fake.health.to_string(),
            "guid" => fake.guid.to_string(),
            other => fake
                .properties
                .get(other)
                .cloned()
                .unwrap_or_else(|| "-".to_owned()),
        };
        Ok(Property::new(value, Source::None))
    }

    fn filesystems(
        &self,
        root: Option<&DatasetName>,
        properties: &[&str],
    ) -> Result<Vec<Dataset>, ZfsError> {
        let shown = root.map_or_else(|| "-".to_owned(), ToString::to_string);
        self.record(format!("filesystems {shown} {}", properties.join(",")));
        let state = self.state.borrow();
        if let Some(root) = root
            && !state.datasets.contains_key(root)
        {
            return Err(ZfsError::Missing(root.to_string()));
        }
        Ok(state
            .datasets
            .keys()
            .filter(|dataset| {
                root.is_none_or(|root| {
                    dataset.as_str() == root.as_str()
                        || dataset
                            .as_str()
                            .strip_prefix(root.as_str())
                            .is_some_and(|rest| rest.starts_with('/'))
                })
            })
            .map(|dataset| Dataset {
                name: dataset.clone(),
                properties: properties
                    .iter()
                    .map(|property| {
                        (
                            (*property).to_owned(),
                            FakeZfs::resolve(&state, dataset, property),
                        )
                    })
                    .collect(),
            })
            .collect())
    }

    fn snapshots(&self, dataset: &DatasetName) -> Result<Vec<Snapshot>, ZfsError> {
        self.record(format!("snapshots {dataset}"));
        let state = self.state.borrow();
        if !state.datasets.contains_key(dataset) {
            return Err(ZfsError::Missing(dataset.to_string()));
        }
        Ok(state
            .snapshots
            .iter()
            .filter(|(snapshot, _)| {
                let of = snapshot.dataset();
                of.as_str() == dataset.as_str()
                    || of
                        .as_str()
                        .strip_prefix(dataset.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .map(|(name, creation)| Snapshot {
                name: name.clone(),
                creation: *creation,
            })
            .collect())
    }

    fn property(&self, dataset: &DatasetName, property: &str) -> Result<Property, ZfsError> {
        self.record(format!("property {dataset} {property}"));
        let state = self.state.borrow();
        if !state.datasets.contains_key(dataset) {
            return Err(ZfsError::Missing(dataset.to_string()));
        }
        Ok(FakeZfs::resolve(&state, dataset, property))
    }

    fn set(&self, dataset: &DatasetName, property: &str, value: &str) -> Result<(), ZfsError> {
        self.record(format!("set {dataset} {property}={value}"));
        let mut state = self.state.borrow_mut();
        let local = state
            .datasets
            .get_mut(dataset)
            .ok_or_else(|| ZfsError::Missing(dataset.to_string()))?;
        local.insert(property.to_owned(), value.to_owned());
        Ok(())
    }

    fn inherit(&self, dataset: &DatasetName, property: &str) -> Result<(), ZfsError> {
        self.record(format!("inherit {dataset} {property}"));
        let mut state = self.state.borrow_mut();
        let local = state
            .datasets
            .get_mut(dataset)
            .ok_or_else(|| ZfsError::Missing(dataset.to_string()))?;
        local.remove(property);
        Ok(())
    }

    fn set_pool_property(
        &self,
        pool: &PoolName,
        property: &str,
        value: &str,
    ) -> Result<(), ZfsError> {
        self.record(format!("set_pool_property {pool} {property}={value}"));
        changes::set_pool_property(&mut self.state.borrow_mut(), pool, property, value)
    }

    fn snapshot_property(
        &self,
        snapshot: &SnapshotName,
        property: &str,
    ) -> Result<Property, ZfsError> {
        self.record(format!("snapshot_property {snapshot} {property}"));
        let state = self.state.borrow();
        let creation = state
            .snapshots
            .get(snapshot)
            .ok_or_else(|| ZfsError::Missing(snapshot.to_string()))?;
        Ok(match property {
            "creation" => Property::new(creation.to_string(), Source::None),
            _ => FakeZfs::resolve(&state, snapshot.dataset(), property),
        })
    }

    fn own_properties(&self, dataset: &DatasetName) -> Result<Vec<(String, Property)>, ZfsError> {
        self.record(format!("own_properties {dataset}"));
        changes::own_properties(&self.state.borrow(), dataset)
    }

    fn create_snapshot(&self, snapshot: &SnapshotName) -> Result<(), ZfsError> {
        self.record(format!("create_snapshot {snapshot}"));
        changes::create_snapshot(&mut self.state.borrow_mut(), snapshot)
    }

    fn rollback(&self, snapshot: &SnapshotName) -> Result<(), ZfsError> {
        self.record(format!("rollback {snapshot}"));
        changes::rollback(&mut self.state.borrow_mut(), snapshot)
    }

    fn clone_snapshot(
        &self,
        snapshot: &SnapshotName,
        target: &DatasetName,
        properties: &[(String, String)],
    ) -> Result<(), ZfsError> {
        self.record(format!(
            "clone_snapshot {snapshot} {target} {}",
            shown(properties)
        ));
        changes::clone_snapshot(&mut self.state.borrow_mut(), snapshot, target, properties)
    }

    fn promote(&self, dataset: &DatasetName) -> Result<(), ZfsError> {
        self.record(format!("promote {dataset}"));
        changes::promote(&mut self.state.borrow_mut(), dataset)
    }

    fn send_receive(
        &self,
        snapshot: &SnapshotName,
        target: &DatasetName,
        properties: &[(String, String)],
    ) -> Result<(), ZfsError> {
        self.record(format!(
            "send_receive {snapshot} {target} {}",
            shown(properties)
        ));
        changes::send_receive(&mut self.state.borrow_mut(), snapshot, target, properties)
    }

    fn inherit_key(&self, dataset: &DatasetName) -> Result<(), ZfsError> {
        self.record(format!("inherit_key {dataset}"));
        changes::inherit_key(&mut self.state.borrow_mut(), dataset)
    }

    fn diff(
        &self,
        from: &SnapshotName,
        to: Option<&SnapshotName>,
    ) -> Result<Vec<Difference>, ZfsError> {
        let shown = to.map_or_else(|| "-".to_owned(), ToString::to_string);
        self.record(format!("diff {from} {shown}"));
        changes::diff(&self.state.borrow(), from, to)
    }
}

/// `properties` as a call shows them: `name=value,...`.
fn shown(properties: &[(String, String)]) -> String {
    properties
        .iter()
        .map(|(property, value)| format!("{property}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests;
