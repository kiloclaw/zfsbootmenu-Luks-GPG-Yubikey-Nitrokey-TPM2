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
use zbm_zfs::{Dataset, Health, Pool, Property, Snapshot, Source, Version, Zfs, ZfsError};

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
}

/// The machine the fake describes.
#[derive(Debug, Default)]
struct State {
    pools: BTreeMap<PoolName, FakePool>,
    datasets: BTreeMap<DatasetName, BTreeMap<String, String>>,
    snapshots: BTreeMap<SnapshotName, u64>,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn machine() -> FakeZfs {
        FakeZfs::new()
            .pool("zroot")
            .filesystem("zroot/ROOT", &[("org.zfsbootmenu:commandline", "quiet")])
            .filesystem(
                "zroot/ROOT/void",
                &[("mountpoint", "/"), ("canmount", "noauto")],
            )
            .filesystem("zroot/home", &[])
            .snapshot("zroot/ROOT/void@first", 100)
            .snapshot("zroot/home@x", 200)
            .bootfs("zroot", "zroot/ROOT/void")
    }

    fn dataset(text: &str) -> DatasetName {
        text.parse().unwrap()
    }

    #[test]
    fn properties_are_local_inherited_default_or_unset() {
        let zfs = machine();
        let void = dataset("zroot/ROOT/void");

        assert_eq!(
            zfs.property(&void, "mountpoint").unwrap(),
            Property::new("/", Source::Local)
        );
        assert_eq!(
            zfs.property(&void, "org.zfsbootmenu:commandline").unwrap(),
            Property::new("quiet", Source::Inherited("zroot/ROOT".to_owned()))
        );
        assert_eq!(
            zfs.property(&dataset("zroot/home"), "mountpoint").unwrap(),
            Property::new("/zroot/home", Source::Default)
        );
        assert_eq!(
            zfs.property(&void, "org.zfsbootmenu:kernel")
                .unwrap()
                .value(),
            None
        );
        assert_eq!(
            zfs.property(&dataset("zroot/ROOT"), "canmount").unwrap(),
            Property::new("on", Source::Default)
        );
        assert!(matches!(
            zfs.property(&dataset("zroot/none"), "mountpoint"),
            Err(ZfsError::Missing(_))
        ));
    }

    #[test]
    fn listings_cover_a_subtree() {
        let zfs = machine();
        let names = |root: Option<&DatasetName>| -> Vec<String> {
            zfs.filesystems(root, &["mountpoint"])
                .unwrap()
                .into_iter()
                .map(|dataset| dataset.name.to_string())
                .collect()
        };

        assert_eq!(
            names(Some(&dataset("zroot/ROOT"))),
            ["zroot/ROOT", "zroot/ROOT/void"]
        );
        assert_eq!(names(None).len(), 4);
        let snapshots = zfs.snapshots(&dataset("zroot/ROOT")).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].creation, 100);
        assert!(zfs.filesystems(Some(&dataset("tank")), &[]).is_err());
    }

    #[test]
    fn missing_datasets_and_another_release() {
        let zfs = FakeZfs::default().release("zfs-2.2.7", "").pool("zroot");
        let none = dataset("zroot/none");

        assert_eq!(zfs.version().unwrap().release(), Some((2, 2)));
        assert!(zfs.snapshots(&none).is_err());
        assert!(zfs.set(&none, "canmount", "on").is_err());
        assert!(zfs.inherit(&none, "canmount").is_err());
        assert!(
            zfs.pool_property(&"tank".parse().unwrap(), "readonly")
                .is_err()
        );
        let zroot: PoolName = "zroot".parse().unwrap();
        assert_eq!(zfs.pool_property(&zroot, "bootfs").unwrap().value, "-");
        assert_eq!(zfs.pool_property(&zroot, "health").unwrap().value, "ONLINE");
        assert_eq!(zfs.pool_property(&zroot, "guid").unwrap().value, "1000");
        assert_eq!(zfs.pool_property(&zroot, "feature@x").unwrap().value, "-");
    }

    #[test]
    #[should_panic(expected = "FakeZfs has no pool tank")]
    fn a_builder_for_a_missing_pool_is_a_mistake() {
        let _ = FakeZfs::new().bootfs("tank", "tank/ROOT");
    }

    #[test]
    fn pools_and_changes_are_recorded() {
        let zfs = machine()
            .pool("tank")
            .read_only("tank")
            .pool_setting("tank", "feature@encryption", "active")
            .health("tank", Health::Degraded);
        let pools = zfs.pools().unwrap();
        let tank: PoolName = "tank".parse().unwrap();

        assert_eq!(pools.len(), 2);
        assert!(pools[0].read_only && pools[0].health == Health::Degraded);
        assert_eq!(
            zfs.pool_property(&tank, "feature@encryption")
                .unwrap()
                .value,
            "active"
        );
        assert_eq!(zfs.pool_property(&tank, "readonly").unwrap().value, "on");
        let void = dataset("zroot/ROOT/void");
        zfs.set(&void, "org.zfsbootmenu:kernel", "6.1").unwrap();
        assert_eq!(
            zfs.property(&void, "org.zfsbootmenu:kernel").unwrap().value,
            "6.1"
        );
        zfs.inherit(&void, "org.zfsbootmenu:kernel").unwrap();
        assert_eq!(
            zfs.property(&void, "org.zfsbootmenu:kernel").unwrap().value,
            "-"
        );
        assert_eq!(zfs.version().unwrap().release(), Some((2, 4)));
        assert_eq!(
            zfs.calls()[..3],
            [
                "pools",
                "pool_property tank feature@encryption",
                "pool_property tank readonly"
            ]
        );
    }
}
