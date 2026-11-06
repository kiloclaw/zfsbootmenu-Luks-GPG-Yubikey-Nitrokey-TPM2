//! Tests of the readers of `zpool status`, `zpool import` and `zdb -e -C`
//! on hand-written cases; `tests/adapter.rs` reads whole outputs.

use pretty_assertions::assert_eq;

use super::*;

fn names(vdevs: &[Vdev]) -> Vec<&str> {
    vdevs.iter().map(|vdev| vdev.name.get().as_str()).collect()
}

#[test]
fn cache_devices_and_spares_of_a_pool_not_imported_have_no_state() {
    let listing = "  pool: tank\n    id: 7\n state: ONLINE\nconfig:\n\n\
                   \ttank        ONLINE\n\t  vdb       ONLINE\n\
                   \tcache\n\t  vdc\n\tspares\n\t  vdd\n\tunknown\n";

    let pools = importable(listing).unwrap();

    let vdevs = &pools[0].vdevs;
    assert_eq!(names(vdevs), ["vdb", "vdc", "vdd"]);
    let classes: Vec<_> = vdevs.iter().map(|vdev| vdev.class).collect();
    assert_eq!(
        classes,
        [VdevClass::Data, VdevClass::Cache, VdevClass::Spare]
    );
    assert_eq!(vdevs[0].state, Some(Health::Online));
    assert_eq!(vdevs[1].state, None);
    assert!(vdevs.iter().all(|vdev| vdev.errors.is_none()));
    assert_eq!(pools[0].status, None);
    assert!(!pools[0].foreign);
}

#[test]
fn the_kind_of_a_vdev_is_known_from_its_name_and_children() {
    let rows = [
        "tank  ONLINE",
        "   ",
        "  raidz2-0  ONLINE 0 0 0",
        "    spare-1  ONLINE 0 0 0",
        "      sda  ONLINE 0 0 0",
        "      replacing-0  ONLINE 0 0 0",
        "        sdb  ONLINE 0 0 0",
        "  draid1:2d:4c:0s-1  ONLINE 0 0 0",
        "    sdc  ONLINE 0 0 0",
        "  mirror-2",
        "  odd-3  ONLINE 0 0 0",
        "    sdd  ONLINE 0 0 0",
    ];

    let tree = vdev_tree(&rows);

    let kinds: Vec<_> = tree.iter().map(|vdev| vdev.kind).collect();
    assert_eq!(
        kinds,
        [
            VdevKind::Raidz,
            VdevKind::Draid,
            VdevKind::Device,
            VdevKind::Other
        ]
    );
    let spare = &tree[0].children[0];
    assert_eq!(spare.kind, VdevKind::Spare);
    assert_eq!(spare.children[1].kind, VdevKind::Replacing);
    assert_eq!(names(&spare.children[1].children), ["sdb"]);
    assert_eq!(tree[2].state, None);
}

#[test]
fn a_tree_deeper_than_any_pool_is_cut_short() {
    let rows: Vec<String> = (1..40)
        .map(|level| format!("{}d{level}  ONLINE", " ".repeat(level * 2)))
        .collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();

    let tree = vdev_tree(&rows);

    let mut levels = 0;
    let mut below = tree.as_slice();
    while let Some(vdev) = below.last() {
        levels += 1;
        below = &vdev.children;
    }
    assert_eq!(levels, MAX_LEVELS);
    assert_eq!(tree.len(), 1);
}

#[test]
fn notes_follow_the_state_and_the_counts() {
    let vdev = row_vdev(
        "sda",
        &["FAULTED", "3", "1", "0", "too", "many", "errors"],
        VdevClass::Data,
    );
    assert_eq!(
        vdev.errors,
        Some(VdevErrors {
            read: 3,
            write: 1,
            cksum: 0
        })
    );
    assert_eq!(vdev.note.unwrap().get(), "too many errors");
    let human = row_vdev("sda", &["ONLINE", "1.2K", "0", "0"], VdevClass::Data);
    assert_eq!(human.errors, None);
    assert_eq!(human.note.unwrap().get(), "1.2K 0 0");
    let bare = row_vdev("sda", &[], VdevClass::Log);
    assert_eq!((bare.state, bare.errors, bare.note), (None, None, None));
}

#[test]
fn a_pool_without_a_name_an_identifier_or_a_state_is_left_out() {
    assert_eq!(
        status("  pool: tank/x\n state: ONLINE\n")
            .unwrap_err()
            .reason,
        "invalid pool name"
    );
    assert_eq!(status("  pool: tank\n").unwrap_err().reason, "no state");
    assert_eq!(
        importable("  pool: tank\n state: ONLINE\n")
            .unwrap_err()
            .reason,
        "no numeric identifier"
    );
    assert_eq!(
        importable("  pool: tank\n    id: x\n state: ONLINE\n")
            .unwrap_err()
            .reason,
        "no numeric identifier"
    );
    assert!(status("").unwrap().is_empty());
    assert!(importable("no pools available\n").unwrap().is_empty());
    let one_bad = "  pool: tank/x\n state: ONLINE\n\n  pool: rpool\n state: ONLINE\n";
    let read = status(one_bad).unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].name.as_str(), "rpool");
    assert_eq!(read[0].errors, None);
    assert!(read[0].vdevs.is_empty());
}

#[test]
fn errors_are_counted_when_the_line_says_how_many() {
    assert_eq!(data_errors("No known data errors"), Some(0));
    assert_eq!(data_errors("12 data errors, use '-v' for a list"), Some(12));
    assert_eq!(
        data_errors("List of errors unavailable: permission denied"),
        None
    );
    assert_eq!(data_errors(""), None);
}

#[test]
fn a_pool_imported_elsewhere_is_foreign() {
    assert!(is_foreign("The pool was last accessed by another system."));
    assert!(is_foreign(
        "The pool is currently imported by another system."
    ));
    assert!(!is_foreign(
        "One or more devices are missing from the system."
    ));
}

#[test]
fn a_pool_that_may_be_active_elsewhere_is_foreign() {
    let listing = |key: &str| {
        format!(
            "  pool: tank\n    id: 7\n state: UNAVAIL\n\
             status: The pool metadata is corrupted.\n\
             {key}: The pool cannot be imported due to damaged devices or data.\n\
             \tThe pool may be active on another system, but can be imported using\n\
             \tthe '-f' flag.\nconfig:\n\n\ttank  UNAVAIL\n"
        )
    };

    for key in ["action", " comment"] {
        let pools = importable(&listing(key)).unwrap();
        assert!(pools[0].foreign, "{key}");
    }
    let said_in_a_comment = "  pool: tank\n    id: 7\n state: ONLINE\n\
                             comment: The pool may be active on another system\n";
    assert!(!importable(said_in_a_comment).unwrap()[0].foreign);
}

#[test]
fn the_host_is_read_at_the_level_of_the_first_key() {
    let config = "\nMOS Configuration:\n        version: 5000\n        name: 'tank'\n\
                  \x20       hostid: 0\n        vdev_tree:\n            hostname: 'nested'\n";
    assert_eq!(pool_config(config).unwrap(), PoolHost::default());
    let config = "MOS Configuration:\n        hostid: 255\n        hostname: 'it''s'\n\
                  \x20       name: 'tank'\n        hostid: 1\n";
    let host = pool_config(config).unwrap();
    assert_eq!(host.hostid.unwrap().as_str(), "000000ff");
    assert_eq!(host.hostname.unwrap().get(), "it''s");
    let config = "MOS Configuration:\n        name: 'tank'\n        hostname: unquoted\n";
    assert_eq!(pool_config(config).unwrap(), PoolHost::default());
}

#[test]
fn a_configuration_of_another_shape_is_an_error() {
    let reason = |text: &str| pool_config(text).unwrap_err().reason;
    assert_eq!(reason(""), "no configuration");
    assert_eq!(
        reason("zdb: note: something\n        name: 'tank'\n"),
        "no configuration"
    );
    assert_eq!(
        reason("MOS Configuration:\n    version: 5000\n        name: 'tank'\n"),
        "no pool name"
    );
    assert_eq!(
        reason("MOS Configuration:\n        name: 'tank'\n        hostid: many\n"),
        "invalid host ID"
    );
}
