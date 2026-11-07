//! The pools of [`FakeZfs`] as `zpool status`, `zpool import` and `zdb`
//! describe them, and the builders of their settings: a pool's vdevs are
//! its devices unless a test gives a tree, such as
//! [`FakeZfs::degraded_mirror`]; the pools that could be imported are
//! those a test adds, such as [`FakeZfs::foreign`], and those exported.
//!
//! ```
//! use zbm_testkit::zfs::FakeZfs;
//! use zbm_zfs::{Health, Zfs};
//!
//! let zfs = FakeZfs::new()
//!     .pool("zroot")
//!     .degraded_mirror("zroot")
//!     .foreign("tank", 7, "deadbeef", "other", 1_792_667_543)
//!     .exported_twice("data");
//! let status = zfs.status().unwrap();
//! assert_eq!(status[0].state, Health::Degraded);
//! let importable = zfs.importable().unwrap();
//! assert!(importable[0].foreign);
//! assert_eq!(importable[1].name, importable[2].name);
//! let host = zfs.pool_host(7).unwrap();
//! assert_eq!(host.hostid.unwrap().as_str(), "deadbeef");
//! assert_eq!(host.written, Some(1_792_667_543));
//! ```

use std::collections::BTreeMap;

use zbm_types::{HostId, PoolName, Untrusted};
use zbm_zfs::{
    Checkpoint, Health, Importable, PoolHost, PoolStatus, Vdev, VdevClass, VdevErrors, VdevKind,
    ZfsError,
};

use super::{FakePool, FakeZfs, State, name};

/// What the fake says of its pools beyond their properties.
#[derive(Debug, Default)]
pub(super) struct Described {
    /// The vdevs of imported pools that a test gave.
    vdevs: BTreeMap<PoolName, Vec<Vdev>>,
    /// The `status:` and `action:` of imported pools with a problem.
    problems: BTreeMap<PoolName, (String, String)>,
    /// The pools that could be imported, in the order listed.
    pub(super) importable: Vec<Importable>,
    /// The host of each pool not imported that a test named, by its
    /// numeric identifier.
    hosts: BTreeMap<u64, PoolHost>,
    /// When the checkpoint of each imported pool that a test named was
    /// taken, as `zpool status` prints it.
    checkpoints: BTreeMap<PoolName, String>,
}

/// `vdev` and the vdevs below it without error counts, as `zpool import`
/// lists them.
fn listed(vdev: &Vdev) -> Vdev {
    Vdev {
        errors: None,
        children: vdev.children.iter().map(listed).collect(),
        ..vdev.clone()
    }
}

/// A device without errors in `class`.
fn device(path: &str, state: Health, class: VdevClass) -> Vdev {
    Vdev {
        name: Untrusted::new(path.to_owned()),
        kind: VdevKind::Device,
        class,
        state: Some(state),
        errors: Some(VdevErrors::default()),
        note: None,
        children: Vec::new(),
    }
}

impl FakeZfs {
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

    /// Gives `pool` the top-level vdevs `vdevs`, which `status` shows
    /// instead of its devices.
    #[must_use]
    pub fn vdevs(self, pool: &str, vdevs: Vec<Vdev>) -> FakeZfs {
        self.state
            .borrow_mut()
            .described
            .vdevs
            .insert(name(pool), vdevs);
        self
    }

    /// Makes `pool` a degraded mirror of two disks, the second `UNAVAIL`
    /// with its label missing, as `zpool status` says of it.
    #[must_use]
    pub fn degraded_mirror(self, pool: &str) -> FakeZfs {
        let mut missing = device("9876543210987654321", Health::Unavailable, VdevClass::Data);
        missing.note = Some(Untrusted::new(
            "was /dev/disk/by-id/ata-DISK-B-part1".to_owned(),
        ));
        let mirror = Vdev {
            name: Untrusted::new("mirror-0".to_owned()),
            kind: VdevKind::Mirror,
            class: VdevClass::Data,
            state: Some(Health::Degraded),
            errors: Some(VdevErrors::default()),
            note: None,
            children: vec![
                device(
                    "/dev/disk/by-id/ata-DISK-A-part1",
                    Health::Online,
                    VdevClass::Data,
                ),
                missing,
            ],
        };
        let problem = (
            "One or more devices could not be used because the label is missing or invalid.  \
             Sufficient replicas exist for the pool to continue functioning in a degraded state."
                .to_owned(),
            "Replace the device using 'zpool replace'.".to_owned(),
        );
        self.state
            .borrow_mut()
            .described
            .problems
            .insert(name(pool), problem);
        self.health(pool, Health::Degraded)
            .vdevs(pool, vec![mirror])
    }

    /// Adds `pool`, with the numeric identifier `id`, to the pools that
    /// could be imported: online, on one disk.
    #[must_use]
    pub fn exported(self, pool: &str, id: u64) -> FakeZfs {
        self.state
            .borrow_mut()
            .described
            .importable
            .push(Importable {
                name: name(pool),
                id,
                state: Health::Online,
                foreign: false,
                active: false,
                status: None,
                action: Some(Untrusted::new(
                    "The pool can be imported using its name or numeric identifier.".to_owned(),
                )),
                vdevs: vec![Vdev {
                    errors: None,
                    ..device(&format!("disk-{id}"), Health::Online, VdevClass::Data)
                }],
            });
        self
    }

    /// Adds two pools named `pool` that could be imported, told apart only
    /// by their identifiers, 1 and 2.
    #[must_use]
    pub fn exported_twice(self, pool: &str) -> FakeZfs {
        self.exported(pool, 1).exported(pool, 2)
    }

    /// Adds `pool`, with the numeric identifier `id`, to the pools that
    /// could be imported, last used by the host `hostname` with the host
    /// ID `hostid`, which last wrote it at `written` seconds.
    #[must_use]
    pub fn foreign(
        self,
        pool: &str,
        id: u64,
        hostid: &str,
        hostname: &str,
        written: u64,
    ) -> FakeZfs {
        let zfs = self.exported(pool, id);
        {
            let mut state = zfs.state.borrow_mut();
            let described = &mut state.described;
            if let Some(listed) = described.importable.last_mut() {
                listed.foreign = true;
                listed.status = Some(Untrusted::new(
                    "The pool was last accessed by another system.".to_owned(),
                ));
                listed.action = Some(Untrusted::new(
                    "The pool can be imported using its name or numeric identifier and the \
                     '-f' flag."
                        .to_owned(),
                ));
            }
            described.hosts.insert(
                id,
                PoolHost {
                    hostid: Some(name::<HostId>(hostid)),
                    hostname: Some(Untrusted::new(hostname.to_owned())),
                    written: Some(written),
                },
            );
        }
        zfs
    }

    /// Makes the pools named `pool` that could be imported imported by
    /// another system now, which `zpool import -f` refuses to take.
    #[must_use]
    pub fn active(self, pool: &str) -> FakeZfs {
        {
            let mut state = self.state.borrow_mut();
            let pool: PoolName = name(pool);
            for listed in &mut state.described.importable {
                if listed.name == pool {
                    listed.foreign = true;
                    listed.active = true;
                    listed.status = Some(Untrusted::new(
                        "The pool is currently imported by another system.".to_owned(),
                    ));
                }
            }
        }
        self
    }

    /// Says that the checkpoint of `pool`, which
    /// [`FakeZfs::pool_setting`] gives with its space, was taken at
    /// `created`, as `zpool status` prints it (`Fri Nov  6 12:00:00
    /// 2026`).
    #[must_use]
    pub fn checkpoint_created(self, pool: &str, created: &str) -> FakeZfs {
        self.state
            .borrow_mut()
            .described
            .checkpoints
            .insert(name(pool), created.to_owned());
        self
    }

    pub(super) fn with_pool(self, pool: &str, change: impl FnOnce(&mut FakePool)) -> FakeZfs {
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
}

/// The status of each imported pool: its vdevs, or a disk for each of its
/// devices, and its problem, if a test gave one.
pub(super) fn status(state: &State) -> Vec<PoolStatus> {
    let described = &state.described;
    state
        .pools
        .iter()
        .map(|(pool, fake)| {
            let problem = described.problems.get(pool);
            let text = |text: &String| Untrusted::new(text.clone());
            PoolStatus {
                name: pool.clone(),
                state: fake.health.clone(),
                status: problem.map(|(status, _)| text(status)),
                action: problem.map(|(_, action)| text(action)),
                errors: Some(0),
                vdevs: described.vdevs.get(pool).cloned().unwrap_or_else(|| {
                    fake.devices
                        .iter()
                        .map(|path| device(path, Health::Online, VdevClass::Data))
                        .collect()
                }),
            }
        })
        .collect()
}

/// Lists `pool`, just exported, with `fake`'s identifier, health and
/// vdevs, among the pools that could be imported.
pub(super) fn list_exported(described: &mut Described, pool: &PoolName, fake: &FakePool) {
    let vdevs = described.vdevs.remove(pool).unwrap_or_else(|| {
        fake.devices
            .iter()
            .map(|path| device(path, Health::Online, VdevClass::Data))
            .collect()
    });
    described.problems.remove(pool);
    described.importable.push(Importable {
        name: pool.clone(),
        id: fake.guid,
        state: fake.health.clone(),
        foreign: false,
        active: false,
        status: None,
        action: Some(Untrusted::new(
            "The pool can be imported using its name or numeric identifier.".to_owned(),
        )),
        vdevs: vdevs.iter().map(listed).collect(),
    });
}

/// Fails as [`FakeZfs::failing`] planned for the read `operation`, which
/// runs `command`.
pub(super) fn planned(state: &State, operation: &str, command: &str) -> Result<(), ZfsError> {
    match state.failures.get(operation) {
        Some(message) => Err(ZfsError::Failed {
            command: command.to_owned(),
            status: 1,
            message: message.clone(),
        }),
        None => Ok(()),
    }
}

/// The checkpoint of the imported `pool`: the `checkpoint` a test set
/// ([`FakeZfs::pool_setting`]), its space in bytes, with the time a test
/// gave ([`FakeZfs::checkpoint_created`]).
pub(super) fn checkpoint(state: &State, pool: &PoolName) -> Result<Option<Checkpoint>, ZfsError> {
    let fake = state.pools.get(pool).ok_or_else(|| ZfsError::Failed {
        command: format!("zpool status -p {pool}"),
        status: 1,
        message: format!("cannot open '{pool}': no such pool"),
    })?;
    Ok(fake
        .properties
        .get("checkpoint")
        .and_then(|space| space.parse().ok())
        .map(|space| Checkpoint {
            created: state
                .described
                .checkpoints
                .get(pool)
                .map(|created| Untrusted::new(created.clone())),
            space: Some(space),
            discarding: false,
        }))
}

/// The host of the pool that could be imported with the numeric
/// identifier `id`, as `zdb -e -C -u` reads it: the one a test named, or none
/// recorded.
pub(super) fn pool_host(state: &State, id: u64) -> Result<PoolHost, ZfsError> {
    let described = &state.described;
    if !described.importable.iter().any(|listed| listed.id == id) {
        return Err(ZfsError::Failed {
            command: format!("zdb -e -C -u {id}"),
            status: 1,
            message: format!("zdb: can't open '{id}': No such file or directory"),
        });
    }
    Ok(described.hosts.get(&id).cloned().unwrap_or_default())
}

#[cfg(test)]
mod tests;
