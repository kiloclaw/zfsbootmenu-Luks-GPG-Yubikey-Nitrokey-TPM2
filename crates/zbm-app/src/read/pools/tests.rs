//! Tests of the pools the snapshot lists: imported, with their devices,
//! and those the scan finds.

use pretty_assertions::assert_eq;
use zbm_testkit::zfs::FakeZfs;
use zbm_types::HostId;

use super::*;

/// The name of `pool`, its state and the names of its devices, with the
/// devices below them in brackets.
fn shape(pool: &Pool) -> (String, Health, Vec<String>) {
    fn named(device: &Device) -> String {
        let below: Vec<String> = device.children.iter().map(named).collect();
        let state = device.state.map_or("-", DeviceState::as_str);
        if below.is_empty() {
            format!("{} {state}", device.name.get())
        } else {
            format!("{} {state} [{}]", device.name.get(), below.join(", "))
        }
    }
    (
        pool.name.to_string(),
        pool.health,
        pool.devices.iter().map(named).collect(),
    )
}

#[test]
fn imported_pools_say_their_health_mode_checkpoint_and_identifier() {
    let zfs = FakeZfs::new()
        .pool("tank")
        .pool("zroot")
        .health("tank", zbm_zfs::Health::Suspended)
        .read_only("tank")
        .pool_setting("zroot", "checkpoint", "524288");

    let pools = pools(&zfs).unwrap();

    let summary: Vec<(&str, Health, bool, Option<bool>)> = pools
        .iter()
        .map(|pool| {
            (
                pool.name.as_str(),
                pool.health,
                pool.writable,
                pool.checkpoint,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("tank", Health::Faulted, false, Some(false)),
            ("zroot", Health::Online, true, Some(true)),
        ]
    );
    assert!(
        pools
            .iter()
            .all(|pool| pool.imported && pool.host.is_none())
    );
    assert_eq!(pools[0].id, Some(1000));
    assert_eq!(pools[1].id, Some(1001));
    assert_eq!(health(&zbm_zfs::Health::Removed), Health::Unavailable);
    assert_eq!(health(&zbm_zfs::Health::Degraded), Health::Degraded);
    // One status for every pool, no scan of the devices.
    let calls = zfs.calls();
    assert_eq!(calls.iter().filter(|call| *call == "status").count(), 1);
    assert!(!calls.iter().any(|call| call == "importable"));
}

#[test]
fn a_degraded_pool_keeps_its_devices_states_apart() {
    let zfs = FakeZfs::new().pool("tank").degraded_mirror("tank");

    let pools = pools(&zfs).unwrap();

    let tank = &pools[0];
    assert_eq!(
        shape(tank),
        (
            "tank".to_owned(),
            Health::Degraded,
            vec![
                "mirror-0 DEGRADED [/dev/disk/by-id/ata-DISK-A-part1 ONLINE, \
                 9876543210987654321 UNAVAIL]"
                    .to_owned()
            ]
        )
    );
    let missing = tank.missing_devices();
    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].note.as_ref().map(|note| note.get().as_str()),
        Some("was /dev/disk/by-id/ata-DISK-B-part1")
    );
    assert_eq!(missing[0].errors, Some(DeviceErrors::default()));
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
    assert_eq!(tank.errors, Some(0));
}

#[test]
fn each_device_keeps_the_class_of_its_top_level_vdev() {
    let vdev = |name: &str, class, children| zbm_zfs::Vdev {
        name: zbm_types::Untrusted::new(name.to_owned()),
        kind: zbm_zfs::VdevKind::Device,
        class,
        state: Some(zbm_zfs::Health::Online),
        errors: None,
        note: None,
        children,
    };
    let disk = |name: &str, class| vdev(name, class, Vec::new());
    let classes = [
        (VdevClass::Data, DeviceClass::Data),
        (VdevClass::Dedup, DeviceClass::Dedup),
        (VdevClass::Special, DeviceClass::Special),
        (VdevClass::Log, DeviceClass::Log),
        (VdevClass::Cache, DeviceClass::Cache),
        (VdevClass::Spare, DeviceClass::Spare),
    ];
    let mut vdevs: Vec<zbm_zfs::Vdev> = (classes.iter())
        .map(|&(class, _)| disk(&format!("{class:?}"), class))
        .collect();
    vdevs[3] = vdev(
        "mirror-1",
        VdevClass::Log,
        vec![disk("sdd", VdevClass::Log)],
    );
    let zfs = FakeZfs::new().pool("tank").vdevs("tank", vdevs);

    let pools = pools(&zfs).unwrap();

    let read: Vec<DeviceClass> = pools[0].devices.iter().map(|device| device.class).collect();
    let wanted: Vec<DeviceClass> = classes.iter().map(|&(_, class)| class).collect();
    assert_eq!(read, wanted);
    assert_eq!(pools[0].devices[3].children[0].class, DeviceClass::Log);
}

#[test]
fn a_failed_status_gives_the_pools_without_their_devices() {
    let zfs = FakeZfs::new()
        .pool("tank")
        .degraded_mirror("tank")
        .failing("status", "no status");

    let pools = pools(&zfs).unwrap();

    assert_eq!(
        shape(&pools[0]),
        ("tank".to_owned(), Health::Degraded, vec![])
    );
    assert_eq!(pools[0].status, None);
    assert_eq!(pools[0].errors, None);
}

#[test]
fn the_scan_lists_pools_not_imported_and_asks_the_host_only_of_foreign_ones() {
    let zfs = FakeZfs::new().pool("zroot").exported("backup", 5).foreign(
        "tank",
        7,
        "deadbeef",
        "other\u{1b}[2J",
        1000,
    );

    let found = importable(&zfs).unwrap();

    let names: Vec<(&str, Option<u64>, bool)> = found
        .iter()
        .map(|pool| (pool.name.as_str(), pool.id, pool.imported))
        .collect();
    assert_eq!(
        names,
        [("backup", Some(5), false), ("tank", Some(7), false)]
    );
    assert_eq!(found[0].host, None);
    let host = found[1].host.as_ref().unwrap();
    assert_eq!(host.hostid.as_ref().map(HostId::as_str), Some("deadbeef"));
    // The host name stays as the label has it, drawn printable.
    let hostname = host.hostname.as_ref().unwrap();
    assert_eq!(hostname.printable().to_string(), "other\\x1b[2J");
    assert_eq!(found[1].checkpoint, None);
    assert_eq!(shape(&found[0]).2, ["disk-5 ONLINE"]);
    assert_eq!(found[0].devices[0].errors, None);
    assert_eq!(zfs.calls(), ["importable", "pool_host 7"]);
}

#[test]
fn pools_of_one_name_are_told_apart_by_their_identifier() {
    let zfs = FakeZfs::new().pool("data").exported_twice("data");

    let imported = pools(&zfs).unwrap();
    let found = importable(&zfs).unwrap();
    let all = merged(imported, &found);

    let ids: Vec<(&str, Option<u64>, bool)> = all
        .iter()
        .map(|pool| (pool.name.as_str(), pool.id, pool.imported))
        .collect();
    assert_eq!(
        ids,
        [
            ("data", Some(1000), true),
            ("data", Some(1), false),
            ("data", Some(2), false)
        ]
    );
}

#[test]
fn a_pool_imported_since_the_scan_is_listed_once() {
    let zfs = FakeZfs::new().pool("zroot").pool("tank");
    let imported = pools(&zfs).unwrap();
    let mut stale = imported[1].clone();
    stale.imported = false;

    let all = merged(imported, &[stale]);

    assert_eq!(all.len(), 2);
    assert!(all.iter().all(|pool| pool.imported));
}

#[test]
fn a_failed_scan_or_host_is_no_pool_or_an_unknown_host() {
    let zfs = FakeZfs::new()
        .foreign("tank", 7, "deadbeef", "other", 1000)
        .failing("pool_host", "no label");

    let found = importable(&zfs).unwrap();
    assert_eq!(found[0].host, Some(PoolHost::default()));

    let failed = FakeZfs::new()
        .exported("tank", 7)
        .failing("importable", "no scan");
    assert_eq!(importable(&failed), None);
    assert_eq!(device_state(&zbm_zfs::Health::Other("AVAIL".into())), None);
    assert_eq!(
        device_state(&zbm_zfs::Health::Removed),
        Some(DeviceState::Removed)
    );
    assert_eq!(
        device_state(&zbm_zfs::Health::Offline),
        Some(DeviceState::Offline)
    );
}
