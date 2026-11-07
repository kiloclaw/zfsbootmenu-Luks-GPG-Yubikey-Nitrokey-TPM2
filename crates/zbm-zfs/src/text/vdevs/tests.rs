//! Tests of the readers of `zpool status`, `zpool import` and `zdb -e -C -u`
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

#[test]
fn a_pool_imported_elsewhere_now_is_active_and_one_used_last_is_not() {
    let listing = |status: &str| {
        format!(
            "  pool: tank\n    id: 7\n state: ONLINE\nstatus: {status}\n\
             config:\n\n\ttank  ONLINE\n"
        )
    };

    let active = importable(&listing(
        "The pool is currently imported by another system.",
    ))
    .unwrap();
    let last = importable(&listing("The pool was last accessed by another system.")).unwrap();

    assert!(active[0].foreign && active[0].active);
    assert!(last[0].foreign && !last[0].active);
    let unreadable = "  pool: tank\n    id: 7\n state: UNAVAIL\n\
                      action: The pool cannot be imported.\n\
                      \tThe pool may be active on another system, but can be imported using\n\
                      \tthe '-f' flag.\n";
    let may_be = importable(unreadable).unwrap();
    assert!(may_be[0].foreign && !may_be[0].active);
}

#[test]
fn the_uberblock_says_when_the_pool_was_last_written() {
    let config = "\nMOS Configuration:\n        name: 'tank'\n        hostid: 255\n\
                  \nUberblock:\n\tmagic = 0000000000bab10c\n\ttxg = 1234\n\
                  \ttimestamp = 1792667543 UTC = Fri Oct 23 08:32:23 2026\n\
                  \tcheckpoint_txg = 0\n\n";
    assert_eq!(pool_config(config).unwrap().written, Some(1_792_667_543));
    let before = "Uberblock:\n\ttimestamp = 7 UTC = Thu Jan  1 00:00:07 1970\n\n\
                  MOS Configuration:\n        name: 'tank'\n";
    assert_eq!(pool_config(before).unwrap().written, Some(7));
    let none = "MOS Configuration:\n        name: 'tank'\n";
    assert_eq!(pool_config(none).unwrap().written, None);
    let without = "MOS Configuration:\n        name: 'tank'\nUberblock:\n\ttxg = 4\n\n\
                   \ttimestamp = 9 UTC = later\n";
    assert_eq!(pool_config(without).unwrap().written, None);
    let garbled = "MOS Configuration:\n        name: 'tank'\nUberblock:\n\
                   \ttimestamp = soon UTC = later\n";
    assert_eq!(
        pool_config(garbled).unwrap_err().reason,
        "invalid uberblock time"
    );
}

#[test]
fn a_checkpoint_is_its_time_and_space_or_the_space_left_to_discard() {
    let status = |line: &str| {
        format!(
            "  pool: tank\n state: ONLINE\n{line}config:\n\n\
             \tNAME  STATE  READ WRITE CKSUM\n\ttank  ONLINE  0 0 0\n\n\
             errors: No known data errors\n"
        )
    };

    let created = checkpoint(&status(
        "checkpoint: created Fri Oct 23 08:32:23 2026, consumes 1.50M\n",
    ))
    .unwrap()
    .unwrap();
    let discarding = checkpoint(&status("checkpoint: discarding, 512 remaining.\n"))
        .unwrap()
        .unwrap();

    assert_eq!(
        created.created.as_ref().unwrap().get(),
        "Fri Oct 23 08:32:23 2026"
    );
    assert_eq!(created.space, Some(1_572_864));
    assert!(!created.discarding);
    assert_eq!(
        discarding,
        Checkpoint {
            created: None,
            space: Some(512),
            discarding: true,
        }
    );
    assert_eq!(checkpoint(&status("")), Ok(None));
    let reason = |text: &str| checkpoint(text).unwrap_err().reason;
    assert_eq!(reason(""), "no pool");
    assert_eq!(
        reason(&status("checkpoint: kept since yesterday\n")),
        "unknown checkpoint"
    );
    assert_eq!(
        reason(&status("checkpoint: created today\n")),
        "unknown checkpoint"
    );
    assert_eq!(
        reason(&status("checkpoint: discarding, 1K\n")),
        "unknown checkpoint"
    );
}

#[test]
fn sizes_for_people_are_read_in_bytes_rounded_down() {
    assert_eq!(nicenum("0"), Some(0));
    assert_eq!(nicenum("1023"), Some(1023));
    assert_eq!(nicenum("4K"), Some(4096));
    assert_eq!(nicenum("1.50M"), Some(1_572_864));
    assert_eq!(nicenum("15.3G"), Some(16_428_249_907));
    assert_eq!(nicenum("2E"), Some(2_305_843_009_213_693_952));
    for odd in ["", "M", "1.M", ".5M", "1.5Q", "1k", "x", "16E", "1,5M"] {
        assert_eq!(nicenum(odd), None, "{odd:?}");
    }
}
