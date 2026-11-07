//! The pools of the snapshot (ADR 0016, point 5): the imported ones, with
//! the tree of their devices from one `zpool status`, and those the scan
//! finds that could be imported, with the host of each that another
//! system used last.
//!
//! The devices and the host are read at best: when `zpool status` or `zdb`
//! fails, the pools are there without them, and the failure is logged.

use zbm_zfs::{Importable, Vdev, VdevClass, Zfs, ZfsError};

use crate::model::{Device, DeviceClass, DeviceErrors, DeviceState, Health, Pool, PoolHost};

/// The health of a pool, in the four the menu tells apart.
pub(super) fn health(health: &zbm_zfs::Health) -> Health {
    match health {
        zbm_zfs::Health::Online => Health::Online,
        zbm_zfs::Health::Degraded => Health::Degraded,
        zbm_zfs::Health::Faulted | zbm_zfs::Health::Suspended => Health::Faulted,
        _ => Health::Unavailable,
    }
}

/// The state of a device, which keeps apart what [`health`] folds; `None`
/// for a state the menu does not tell apart, as a spare's `AVAIL`.
fn device_state(health: &zbm_zfs::Health) -> Option<DeviceState> {
    Some(match health {
        zbm_zfs::Health::Online => DeviceState::Online,
        zbm_zfs::Health::Degraded => DeviceState::Degraded,
        zbm_zfs::Health::Faulted | zbm_zfs::Health::Suspended => DeviceState::Faulted,
        zbm_zfs::Health::Offline => DeviceState::Offline,
        zbm_zfs::Health::Unavailable => DeviceState::Unavailable,
        zbm_zfs::Health::Removed => DeviceState::Removed,
        zbm_zfs::Health::Other(_) => return None,
    })
}

/// The class of devices `class` names.
fn device_class(class: VdevClass) -> DeviceClass {
    match class {
        VdevClass::Data => DeviceClass::Data,
        VdevClass::Dedup => DeviceClass::Dedup,
        VdevClass::Special => DeviceClass::Special,
        VdevClass::Log => DeviceClass::Log,
        VdevClass::Cache => DeviceClass::Cache,
        VdevClass::Spare => DeviceClass::Spare,
    }
}

/// The device `vdev` describes, and those below it.
fn device(vdev: &Vdev) -> Device {
    Device {
        name: vdev.name.clone(),
        class: device_class(vdev.class),
        state: vdev.state.as_ref().and_then(device_state),
        errors: vdev.errors.map(|errors| DeviceErrors {
            read: errors.read,
            write: errors.write,
            cksum: errors.cksum,
        }),
        note: vdev.note.clone(),
        children: vdev.children.iter().map(device).collect(),
    }
}

/// The imported pools, each with its checkpoint and, from one `zpool
/// status` of them all, its devices and what `zpool` says is wrong.
///
/// # Errors
///
/// When `zpool list` fails or prints something unexpected; a failed
/// `zpool status` leaves the pools without devices.
pub(crate) fn pools(zfs: &dyn Zfs) -> Result<Vec<Pool>, ZfsError> {
    let listed = zfs.pools()?;
    let statuses = zfs.status().unwrap_or_else(|error| {
        tracing::warn!(%error, "the devices of the pools could not be read");
        Vec::new()
    });
    Ok(listed
        .into_iter()
        .map(|pool| {
            let checkpoint = zfs
                .pool_property(&pool.name, "checkpoint")
                .ok()
                .map(|property| property.value().is_some());
            let status = statuses.iter().find(|status| status.name == pool.name);
            Pool {
                id: Some(pool.guid),
                health: health(&pool.health),
                writable: !pool.read_only,
                imported: true,
                host: None,
                checkpoint,
                devices: status
                    .map(|status| status.vdevs.iter().map(device).collect())
                    .unwrap_or_default(),
                status: status.and_then(|status| status.status.clone()),
                action: status.and_then(|status| status.action.clone()),
                errors: status.and_then(|status| status.errors),
                name: pool.name,
            }
        })
        .collect())
}

/// The pool `listed` describes, which could be imported; `host` is the
/// host another system that used it last is, when it is one.
fn importable_pool(listed: Importable, host: Option<PoolHost>) -> Pool {
    Pool {
        id: Some(listed.id),
        health: health(&listed.state),
        writable: false,
        imported: false,
        host,
        checkpoint: None,
        devices: listed.vdevs.iter().map(device).collect(),
        status: listed.status,
        action: listed.action,
        errors: None,
        name: listed.name,
    }
}

/// The pools that could be imported, as `zpool import` lists them after
/// looking at every device; for each that another system used last, its
/// host, which `zdb -e -C` reads from its configuration, unknown when that
/// fails. `None` when the scan fails, as when it is cancelled, which is
/// logged.
pub(crate) fn importable(zfs: &dyn Zfs) -> Option<Vec<Pool>> {
    let listed = zfs
        .importable()
        .inspect_err(|error| {
            tracing::warn!(%error, "the pools that could be imported could not be listed");
        })
        .ok()?;
    let pools = listed
        .into_iter()
        .map(|listed| {
            let host = listed.foreign.then(|| match zfs.pool_host(listed.id) {
                Ok(host) => PoolHost {
                    hostid: host.hostid,
                    hostname: host.hostname,
                },
                Err(error) => {
                    let pool = &listed.name;
                    tracing::warn!(%error, %pool, "the pool's host could not be read");
                    PoolHost::default()
                }
            });
            importable_pool(listed, host)
        })
        .collect();
    Some(pools)
}

/// The `imported` pools, then those of `importable` that are not among
/// them: a pool the scan found that was imported since is left out.
pub(crate) fn merged(mut imported: Vec<Pool>, importable: &[Pool]) -> Vec<Pool> {
    let ids: Vec<Option<u64>> = imported.iter().map(|pool| pool.id).collect();
    imported.extend(
        importable
            .iter()
            .filter(|pool| pool.id.is_none() || !ids.contains(&pool.id))
            .cloned(),
    );
    imported
}

#[cfg(test)]
mod tests;
