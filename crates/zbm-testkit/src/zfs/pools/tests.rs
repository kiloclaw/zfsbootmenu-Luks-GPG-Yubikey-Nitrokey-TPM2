//! Tests of the pools of [`FakeZfs`] as `zpool status`, `zpool import`
//! and `zdb` describe them.

use pretty_assertions::assert_eq;
use zbm_zfs::Zfs;

use super::*;

fn pool(text: &str) -> PoolName {
    text.parse().unwrap()
}

#[test]
fn a_pool_without_a_tree_shows_its_devices_as_disks() {
    let zfs = FakeZfs::new().pool("zroot").device("zroot", "/dev/dm-3");

    let status = zfs.status().unwrap();

    assert_eq!(status.len(), 1);
    assert_eq!(status[0].state, Health::Online);
    assert_eq!(status[0].status, None);
    assert_eq!(status[0].errors, Some(0));
    assert_eq!(
        status[0].vdevs,
        [device("/dev/dm-3", Health::Online, VdevClass::Data)]
    );
    assert_eq!(zfs.calls(), ["status"]);
}

#[test]
fn a_degraded_mirror_has_one_device_unavailable() {
    let zfs = FakeZfs::new().pool("tank").degraded_mirror("tank");

    let status = zfs.status().unwrap();
    let pools = zfs.pools().unwrap();

    assert_eq!(pools[0].health, Health::Degraded);
    let tank = &status[0];
    assert_eq!(tank.state, Health::Degraded);
    assert!(
        tank.status
            .as_ref()
            .unwrap()
            .get()
            .contains("label is missing")
    );
    assert_eq!(
        tank.action.as_ref().unwrap().get(),
        "Replace the device using 'zpool replace'."
    );
    let mirror = &tank.vdevs[0];
    assert_eq!(mirror.kind, VdevKind::Mirror);
    let health: Vec<_> = mirror.children.iter().map(|d| d.state.clone()).collect();
    assert_eq!(health, [Some(Health::Online), Some(Health::Unavailable)]);
    assert_eq!(
        mirror.children[1].note.as_ref().unwrap().get(),
        "was /dev/disk/by-id/ata-DISK-B-part1"
    );
}

#[test]
fn a_foreign_pool_names_the_host_that_used_it() {
    let zfs = FakeZfs::new().foreign("tank", 42, "deadbeef", "other");

    let listed = zfs.importable().unwrap();
    let host = zfs.pool_host(42).unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, 42);
    assert!(listed[0].foreign);
    assert!(
        listed[0]
            .status
            .as_ref()
            .unwrap()
            .get()
            .contains("another system")
    );
    assert_eq!(listed[0].vdevs[0].errors, None);
    assert_eq!(host.hostid.unwrap().as_str(), "deadbeef");
    assert_eq!(host.hostname.unwrap().get(), "other");
    assert_eq!(zfs.calls(), ["importable", "pool_host 42"]);
}

#[test]
fn pools_with_one_name_are_told_apart_by_their_identifiers() {
    let zfs = FakeZfs::new().pool("data").exported_twice("data");

    let listed = zfs.importable().unwrap();

    let ids: Vec<u64> = listed.iter().map(|listed| listed.id).collect();
    assert_eq!(ids, [1, 2]);
    assert!(listed.iter().all(|listed| listed.name == pool("data")));
    assert!(!listed[0].foreign);
    assert_eq!(zfs.pool_host(2).unwrap(), PoolHost::default());
}

#[test]
fn the_host_of_a_pool_not_listed_is_unknown() {
    let zfs = FakeZfs::new().exported("tank", 9);

    assert_eq!(zfs.pool_host(9).unwrap(), PoolHost::default());
    let error = zfs.pool_host(10).unwrap_err().to_string();
    assert!(error.contains("can't open '10': No such file"), "{error}");
}

#[test]
fn an_exported_pool_can_be_imported() {
    let zfs = FakeZfs::new()
        .pool("zroot")
        .pool("tank")
        .device("tank", "/dev/vdb")
        .degraded_mirror("zroot");
    let guids: Vec<u64> = zfs.pools().unwrap().iter().map(|p| p.guid).collect();

    zfs.export(&pool("tank")).unwrap();
    zfs.export(&pool("zroot")).unwrap();

    let listed = zfs.importable().unwrap();
    let summary: Vec<(&str, u64, Health)> = listed
        .iter()
        .map(|listed| (listed.name.as_str(), listed.id, listed.state.clone()))
        .collect();
    assert_eq!(
        summary,
        [
            ("tank", guids[0], Health::Online),
            ("zroot", guids[1], Health::Degraded),
        ]
    );
    assert_eq!(names(&listed[0].vdevs), ["/dev/vdb"]);
    let mirror = &listed[1].vdevs[0];
    assert_eq!(mirror.kind, VdevKind::Mirror);
    assert!(mirror.errors.is_none() && mirror.children[0].errors.is_none());
    assert!(!listed[1].foreign && listed[1].status.is_none());
    assert_eq!(zfs.pool_host(guids[0]).unwrap(), PoolHost::default());
    assert!(zfs.status().unwrap().is_empty());
}

fn names(vdevs: &[Vdev]) -> Vec<&str> {
    vdevs.iter().map(|vdev| vdev.name.get().as_str()).collect()
}
