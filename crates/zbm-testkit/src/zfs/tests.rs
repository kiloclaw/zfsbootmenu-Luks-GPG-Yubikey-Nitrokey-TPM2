//! Tests of [`FakeZfs`](super::FakeZfs).

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

#[test]
fn pools_are_exported_with_their_datasets_unless_busy() {
    let zfs = machine()
        .pool("tank")
        .device("tank", "/dev/dm-3")
        .device("tank", "/dev/vda2")
        .busy("zroot");
    let tank: PoolName = "tank".parse().unwrap();
    let zroot: PoolName = "zroot".parse().unwrap();

    assert_eq!(zfs.pool_names().unwrap(), [tank.clone(), zroot.clone()]);
    assert_eq!(zfs.devices(&tank).unwrap(), ["/dev/dm-3", "/dev/vda2"]);
    assert!(matches!(
        zfs.export(&zroot),
        Err(ZfsError::Failed { status: 1, .. })
    ));
    zfs.export(&tank).unwrap();
    assert!(matches!(zfs.devices(&tank), Err(ZfsError::Missing(_))));
    assert!(matches!(zfs.export(&tank), Err(ZfsError::Missing(_))));
    assert_eq!(zfs.pool_names().unwrap(), [zroot]);
    assert!(
        zfs.filesystems(None, &[])
            .unwrap()
            .iter()
            .all(|d| d.name.pool() != tank)
    );
    assert_eq!(
        zfs.calls(),
        [
            "pool_names",
            "devices tank",
            "export zroot",
            "export tank",
            "devices tank",
            "export tank",
            "pool_names",
            "filesystems - "
        ]
    );
}
