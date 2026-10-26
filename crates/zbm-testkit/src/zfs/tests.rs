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

fn snapshot(text: &str) -> SnapshotName {
    text.parse().unwrap()
}

fn snapshot_names(zfs: &FakeZfs, of: &str) -> Vec<String> {
    zfs.snapshots(&dataset(of))
        .unwrap()
        .into_iter()
        .map(|found| found.name.to_string())
        .collect()
}

#[test]
fn snapshots_are_taken_and_rolled_back_to() {
    let zfs = machine().snapshot("zroot/ROOT/void@second", 150);
    let first = snapshot("zroot/ROOT/void@first");

    zfs.create_snapshot(&snapshot("zroot/ROOT/void@third"))
        .unwrap();
    assert_eq!(
        zfs.snapshot_property(&snapshot("zroot/ROOT/void@third"), "creation")
            .unwrap()
            .value,
        "201"
    );
    let again = zfs.create_snapshot(&snapshot("zroot/ROOT/void@third"));
    assert!(again.unwrap_err().to_string().contains("already exists"));
    assert!(zfs.create_snapshot(&snapshot("zroot/none@x")).is_err());
    zfs.rollback(&first).unwrap();

    assert_eq!(
        snapshot_names(&zfs, "zroot/ROOT/void"),
        ["zroot/ROOT/void@first"]
    );
    assert_eq!(snapshot_names(&zfs, "zroot/home"), ["zroot/home@x"]);
    assert!(matches!(
        zfs.snapshot_property(&snapshot("zroot/ROOT/void@second"), "creation"),
        Err(ZfsError::Missing(_))
    ));
    assert_eq!(
        zfs.snapshot_property(&first, "mountpoint").unwrap().value,
        "/"
    );
    assert_eq!(
        zfs.calls()[..2],
        [
            "create_snapshot zroot/ROOT/void@third",
            "snapshot_property zroot/ROOT/void@third creation",
        ]
    );
}

#[test]
fn changes_fail_on_read_only_pools_and_as_planned() {
    let zfs = machine().read_only("zroot");
    let first = snapshot("zroot/ROOT/void@first");

    let error = zfs.create_snapshot(&snapshot("zroot/ROOT/void@new"));
    assert!(error.unwrap_err().to_string().contains("pool is read-only"));
    assert!(zfs.rollback(&first).is_err());
    let pool: PoolName = "zroot".parse().unwrap();
    assert!(
        zfs.set_pool_property(&pool, "bootfs", "zroot/home")
            .is_err()
    );

    let zfs = machine().failing("rollback", "cannot rollback: busy");
    let error = zfs.rollback(&first).unwrap_err();
    assert!(
        matches!(error, ZfsError::Failed { message, .. } if message == "cannot rollback: busy")
    );
}

#[test]
fn clones_depend_on_their_origin_until_promoted() {
    let zfs = machine().snapshot("zroot/ROOT/void@second", 150);
    let first = snapshot("zroot/ROOT/void@first");
    let copy = dataset("zroot/ROOT/void_NEW");
    let options = vec![
        ("mountpoint".to_owned(), "/".to_owned()),
        ("canmount".to_owned(), "noauto".to_owned()),
    ];

    zfs.clone_snapshot(&first, &copy, &options).unwrap();
    assert!(zfs.clone_snapshot(&first, &copy, &[]).is_err());
    assert!(
        zfs.clone_snapshot(&first, &dataset("zroot/none/x"), &[])
            .is_err()
    );
    assert_eq!(
        zfs.origin_of("zroot/ROOT/void_NEW").as_deref(),
        Some("zroot/ROOT/void@first")
    );
    assert_eq!(
        zfs.property(&copy, "canmount").unwrap(),
        Property::new("noauto", Source::Local)
    );
    // A later snapshot without clones goes; one with a clone stays.
    zfs.rollback(&first).unwrap();
    let held = machine()
        .snapshot("zroot/ROOT/void@second", 150)
        .origin("zroot/home", "zroot/ROOT/void@second");
    assert!(
        held.rollback(&first)
            .unwrap_err()
            .to_string()
            .contains("dependent clones")
    );

    zfs.promote(&copy).unwrap();

    assert_eq!(
        snapshot_names(&zfs, "zroot/ROOT/void_NEW"),
        ["zroot/ROOT/void_NEW@first"]
    );
    assert_eq!(zfs.origin_of("zroot/ROOT/void_NEW"), None);
    assert_eq!(
        zfs.origin_of("zroot/ROOT/void").as_deref(),
        Some("zroot/ROOT/void_NEW@first")
    );
    assert!(zfs.promote(&copy).is_err());
    assert_eq!(
        zfs.calls()[0],
        "clone_snapshot zroot/ROOT/void@first zroot/ROOT/void_NEW mountpoint=/,canmount=noauto"
    );
}

#[test]
fn copies_are_independent_encryption_roots_until_they_inherit_the_key() {
    let zfs = FakeZfs::new()
        .pool("zroot")
        .filesystem(
            "zroot/ROOT",
            &[("encryptionroot", "zroot/ROOT"), ("keystatus", "available")],
        )
        .filesystem(
            "zroot/ROOT/void",
            &[("mountpoint", "/"), ("encryptionroot", "zroot/ROOT")],
        )
        .snapshot("zroot/ROOT/void@first", 100);
    let copy = dataset("zroot/ROOT/copy");
    let options = vec![("canmount".to_owned(), "noauto".to_owned())];

    zfs.send_receive(&snapshot("zroot/ROOT/void@first"), &copy, &options)
        .unwrap();

    assert_eq!(
        snapshot_names(&zfs, "zroot/ROOT/copy"),
        ["zroot/ROOT/copy@first"]
    );
    assert_eq!(
        zfs.property(&copy, "encryptionroot").unwrap().value,
        "zroot/ROOT/copy"
    );
    assert_eq!(zfs.property(&copy, "mountpoint").unwrap().value, "/");
    let own: Vec<String> = zfs
        .own_properties(&copy)
        .unwrap()
        .into_iter()
        .map(|(name, property)| format!("{name}={}", property.value))
        .collect();
    assert_eq!(own, ["canmount=noauto", "mountpoint=/"]);
    assert_eq!(zfs.origin_of("zroot/ROOT/copy"), None);

    zfs.inherit_key(&copy).unwrap();
    assert_eq!(
        zfs.property(&copy, "encryptionroot").unwrap().value,
        "zroot/ROOT"
    );
    assert!(zfs.inherit_key(&copy).is_err());
    assert!(
        zfs.send_receive(&snapshot("zroot/ROOT/void@first"), &copy, &[])
            .is_err()
    );
    assert!(matches!(
        zfs.own_properties(&dataset("zroot/x")),
        Err(ZfsError::Missing(_))
    ));
}

#[test]
fn bootfs_and_differences() {
    let changed = Difference {
        change: zbm_zfs::DiffChange::Modified,
        file_type: 'F',
        path: "/etc/fstab".to_owned(),
        renamed_to: None,
    };
    let zfs = machine()
        .differences(
            "zroot/ROOT/void@first",
            None,
            std::slice::from_ref(&changed),
        )
        .pool("tank");
    let pool: PoolName = "zroot".parse().unwrap();
    let first = snapshot("zroot/ROOT/void@first");

    zfs.set_pool_property(&pool, "bootfs", "zroot/home")
        .unwrap();
    zfs.set_pool_property(&pool, "comment", "hi").unwrap();
    assert!(
        zfs.set_pool_property(&pool, "bootfs", "zroot/none")
            .is_err()
    );
    assert!(zfs.set_pool_property(&pool, "bootfs", "tank").is_err());
    let none: PoolName = "none".parse().unwrap();
    assert!(zfs.set_pool_property(&none, "comment", "x").is_err());

    assert_eq!(zfs.pools().unwrap()[1].bootfs, Some(dataset("zroot/home")));
    assert_eq!(zfs.pool_property(&pool, "comment").unwrap().value, "hi");
    assert_eq!(zfs.diff(&first, None).unwrap(), [changed]);
    assert_eq!(
        zfs.diff(&first, Some(&snapshot("zroot/home@x"))).unwrap(),
        []
    );
    assert!(zfs.diff(&snapshot("zroot/home@none"), None).is_err());
    assert!(
        zfs.diff(&first, Some(&snapshot("zroot/home@none")))
            .is_err()
    );
    let failing = machine().failing("diff", "not mounted");
    assert!(failing.diff(&first, None).is_err());
}
