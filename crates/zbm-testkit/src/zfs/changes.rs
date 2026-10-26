//! The changes [`FakeZfs`] makes to its machine, as ZFS makes them: a
//! change on a pool imported read-only fails, as does one that a test
//! planned to fail ([`FakeZfs::failing`]). Like `ZfsCli`, a change that
//! fails is [`ZfsError::Failed`] with the message of `zfs`, never
//! [`ZfsError::Missing`].

use std::collections::BTreeMap;

use zbm_types::{DatasetName, PoolName, SnapshotName};
use zbm_zfs::{Difference, Property, Source, ZfsError};

use super::{FakeZfs, State};

/// Properties that describe the fake's machine rather than being set on a
/// dataset, which [`own_properties`] leaves out, as `zfs get -s local`
/// does.
const STATE: &[&str] = &["encryption", "encryptionroot", "keyformat", "keystatus"];

/// A failure of `command` with `message`.
fn failed(command: &str, message: String) -> ZfsError {
    ZfsError::Failed {
        command: command.to_owned(),
        status: 1,
        message,
    }
}

/// Fails as planned with [`FakeZfs::failing`], or when `pool` is missing or
/// imported read-only.
fn check(state: &State, operation: &str, pool: &PoolName, command: &str) -> Result<(), ZfsError> {
    if let Some(message) = state.failures.get(operation) {
        return Err(failed(command, message.clone()));
    }
    match state.pools.get(pool) {
        None => Err(failed(
            command,
            format!("cannot open '{pool}': no such pool"),
        )),
        Some(fake) if fake.read_only => Err(failed(command, "pool is read-only".to_owned())),
        Some(_) => Ok(()),
    }
}

/// Fails unless `snapshot` exists.
fn snapshot_exists(state: &State, snapshot: &SnapshotName, command: &str) -> Result<(), ZfsError> {
    if state.snapshots.contains_key(snapshot) {
        return Ok(());
    }
    Err(failed(
        command,
        format!("cannot open '{snapshot}': dataset does not exist"),
    ))
}

/// Fails when `target` exists or its parent does not.
fn can_create(state: &State, target: &DatasetName, command: &str) -> Result<(), ZfsError> {
    if state.datasets.contains_key(target) {
        return Err(failed(
            command,
            format!("cannot create '{target}': dataset already exists"),
        ));
    }
    let parent = target.as_str().rsplit_once('/').map(|(parent, _)| parent);
    if parent.is_some_and(|parent| !state.datasets.keys().any(|d| d.as_str() == parent)) {
        return Err(failed(
            command,
            format!("cannot create '{target}': parent does not exist"),
        ));
    }
    Ok(())
}

/// `zpool set property=value pool`; `bootfs` must name a dataset of the
/// pool.
pub(super) fn set_pool_property(
    state: &mut State,
    pool: &PoolName,
    property: &str,
    value: &str,
) -> Result<(), ZfsError> {
    let command = format!("zpool set {property}={value} {pool}");
    check(state, "set_pool_property", pool, &command)?;
    let bootfs = if property == "bootfs" {
        let dataset = value
            .parse::<DatasetName>()
            .ok()
            .filter(|dataset| dataset.pool() == *pool && state.datasets.contains_key(dataset))
            .ok_or_else(|| failed(&command, format!("cannot set property for '{pool}'")))?;
        Some(dataset)
    } else {
        None
    };
    if let Some(fake) = state.pools.get_mut(pool) {
        match bootfs {
            Some(dataset) => fake.bootfs = Some(dataset),
            None => {
                fake.properties
                    .insert(property.to_owned(), value.to_owned());
            }
        }
    }
    Ok(())
}

/// The properties set on `dataset`, as local ones.
pub(super) fn own_properties(
    state: &State,
    dataset: &DatasetName,
) -> Result<Vec<(String, Property)>, ZfsError> {
    let local = state
        .datasets
        .get(dataset)
        .ok_or_else(|| ZfsError::Missing(dataset.to_string()))?;
    Ok(local
        .iter()
        .filter(|(property, _)| !STATE.contains(&property.as_str()))
        .map(|(property, value)| {
            (
                property.clone(),
                Property::new(value.clone(), Source::Local),
            )
        })
        .collect())
}

/// `zfs snapshot`: the new snapshot is the latest one.
pub(super) fn create_snapshot(state: &mut State, snapshot: &SnapshotName) -> Result<(), ZfsError> {
    let command = format!("zfs snapshot {snapshot}");
    check(state, "create_snapshot", &snapshot.pool(), &command)?;
    if !state.datasets.contains_key(snapshot.dataset()) {
        return Err(failed(
            &command,
            format!(
                "cannot open '{}': dataset does not exist",
                snapshot.dataset()
            ),
        ));
    }
    if state.snapshots.contains_key(snapshot) {
        return Err(failed(
            &command,
            format!("cannot create snapshot '{snapshot}': dataset already exists"),
        ));
    }
    let creation = state
        .snapshots
        .values()
        .max()
        .map_or(1, |latest| latest + 1);
    state.snapshots.insert(snapshot.clone(), creation);
    Ok(())
}

/// The snapshots of the filesystem of `snapshot` taken after it.
fn later(state: &State, snapshot: &SnapshotName) -> Vec<SnapshotName> {
    let creation = state.snapshots.get(snapshot).copied().unwrap_or_default();
    state
        .snapshots
        .iter()
        .filter(|(other, taken)| other.dataset() == snapshot.dataset() && **taken > creation)
        .map(|(other, _)| other.clone())
        .collect()
}

/// `zfs rollback -r`: destroys the later snapshots, unless one of them has
/// a clone.
pub(super) fn rollback(state: &mut State, snapshot: &SnapshotName) -> Result<(), ZfsError> {
    let command = format!("zfs rollback -r {snapshot}");
    check(state, "rollback", &snapshot.pool(), &command)?;
    snapshot_exists(state, snapshot, &command)?;
    let later = later(state, snapshot);
    if let Some(cloned) = later
        .iter()
        .find(|later| state.origins.values().any(|origin| origin == *later))
    {
        return Err(failed(
            &command,
            format!("cannot destroy '{cloned}': snapshot has dependent clones"),
        ));
    }
    for destroyed in later {
        state.snapshots.remove(&destroyed);
    }
    Ok(())
}

/// Creates `target` with `properties` as local ones.
fn create(state: &mut State, target: &DatasetName, properties: &[(String, String)]) {
    let local = state.datasets.entry(target.clone()).or_default();
    for (property, value) in properties {
        local.insert(property.clone(), value.clone());
    }
}

/// `zfs clone`.
pub(super) fn clone_snapshot(
    state: &mut State,
    snapshot: &SnapshotName,
    target: &DatasetName,
    properties: &[(String, String)],
) -> Result<(), ZfsError> {
    let command = format!("zfs clone {snapshot} {target}");
    check(state, "clone_snapshot", &target.pool(), &command)?;
    snapshot_exists(state, snapshot, &command)?;
    can_create(state, target, &command)?;
    create(state, target, properties);
    state.origins.insert(target.clone(), snapshot.clone());
    Ok(())
}

/// `zfs promote`: the snapshots of the origin's filesystem up to the
/// origin move to the clone, which the origin's filesystem now depends on.
pub(super) fn promote(state: &mut State, dataset: &DatasetName) -> Result<(), ZfsError> {
    let command = format!("zfs promote {dataset}");
    check(state, "promote", &dataset.pool(), &command)?;
    let origin = state
        .origins
        .remove(dataset)
        .ok_or_else(|| failed(&command, "not a cloned filesystem".to_owned()))?;
    let creation = state.snapshots.get(&origin).copied().unwrap_or_default();
    let moved: Vec<(SnapshotName, u64)> = state
        .snapshots
        .iter()
        .filter(|(name, taken)| name.dataset() == origin.dataset() && **taken <= creation)
        .map(|(name, taken)| (name.clone(), *taken))
        .collect();
    let mut renamed = BTreeMap::new();
    for (name, taken) in moved {
        state.snapshots.remove(&name);
        let new = super::name::<SnapshotName>(&format!("{dataset}@{}", name.name()));
        state.snapshots.insert(new.clone(), taken);
        renamed.insert(name, new);
    }
    for origin in state.origins.values_mut() {
        if let Some(new) = renamed.get(origin) {
            *origin = new.clone();
        }
    }
    if let Some(new) = renamed.get(&origin) {
        state.origins.insert(origin.dataset().clone(), new.clone());
    }
    Ok(())
}

/// The raw send of `snapshot` received as `target`: the target has the
/// source's local properties and `properties`, and the snapshot; an
/// encrypted source makes it an encryption root.
pub(super) fn send_receive(
    state: &mut State,
    snapshot: &SnapshotName,
    target: &DatasetName,
    properties: &[(String, String)],
) -> Result<(), ZfsError> {
    let command = format!("zfs send -p -w {snapshot} | zfs recv -u {target}");
    check(state, "send_receive", &target.pool(), &command)?;
    snapshot_exists(state, snapshot, &command)?;
    can_create(state, target, &command)?;
    let source = snapshot.dataset();
    let mut local: Vec<(String, String)> = state
        .datasets
        .get(source)
        .map(|local| {
            local
                .iter()
                .filter(|(property, _)| !STATE.contains(&property.as_str()))
                .map(|(property, value)| (property.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default();
    let root = FakeZfs::resolve(state, source, "encryptionroot");
    if root.value().is_some() {
        let keystatus = FakeZfs::resolve(state, source, "keystatus").value;
        local.push(("encryptionroot".to_owned(), target.to_string()));
        local.push(("keystatus".to_owned(), keystatus));
    }
    local.extend(properties.iter().cloned());
    create(state, target, &local);
    let creation = state.snapshots.get(snapshot).copied().unwrap_or_default();
    let copy = super::name::<SnapshotName>(&format!("{target}@{}", snapshot.name()));
    state.snapshots.insert(copy, creation);
    Ok(())
}

/// `zfs change-key -i`: the encryption root `dataset` takes its parent's.
pub(super) fn inherit_key(state: &mut State, dataset: &DatasetName) -> Result<(), ZfsError> {
    let command = format!("zfs change-key -i {dataset}");
    check(state, "inherit_key", &dataset.pool(), &command)?;
    let own = FakeZfs::resolve(state, dataset, "encryptionroot");
    if own.value != dataset.as_str() {
        return Err(failed(
            &command,
            "Key inheritance can only be performed on encryption roots.".to_owned(),
        ));
    }
    let parent_root = dataset
        .as_str()
        .rsplit_once('/')
        .and_then(|(parent, _)| parent.parse::<DatasetName>().ok())
        .map(|parent| FakeZfs::resolve(state, &parent, "encryptionroot").value)
        .filter(|root| root != "-")
        .ok_or_else(|| failed(&command, "Parent is not encrypted.".to_owned()))?;
    if let Some(local) = state.datasets.get_mut(dataset) {
        local.insert("encryptionroot".to_owned(), parent_root);
    }
    Ok(())
}

/// `zfs diff`: what the test described with [`FakeZfs::differences`].
pub(super) fn diff(
    state: &State,
    from: &SnapshotName,
    to: Option<&SnapshotName>,
) -> Result<Vec<Difference>, ZfsError> {
    let shown = to.map_or_else(|| from.dataset().to_string(), ToString::to_string);
    let command = format!("zfs diff -F -H {from} {shown}");
    if let Some(message) = state.failures.get("diff") {
        return Err(failed(&command, message.clone()));
    }
    snapshot_exists(state, from, &command)?;
    if let Some(to) = to {
        snapshot_exists(state, to, &command)?;
    }
    Ok(state
        .differences
        .get(&(from.clone(), to.cloned()))
        .cloned()
        .unwrap_or_default())
}
