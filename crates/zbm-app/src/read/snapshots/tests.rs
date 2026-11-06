//! Tests of the snapshots read of an environment: the count and the newest
//! for every snapshot of the machine, the whole list on demand.

use pretty_assertions::assert_eq;
use zbm_testkit::zfs::FakeZfs;

use super::*;
use crate::read::environments;

/// How many snapshots the environment of [`machine`] has: B5's largest.
const MANY: u64 = 10_000;

/// A pool with one environment of [`MANY`] snapshots, taken one second
/// apart, the oldest first, and a descendant with one of its own.
fn machine() -> FakeZfs {
    let mut zfs = FakeZfs::new()
        .pool("tank")
        .filesystem("tank/ROOT/be", &[("mountpoint", "/")])
        .filesystem("tank/ROOT/be/home", &[("mountpoint", "/home")])
        .snapshot("tank/ROOT/be/home@mine", 5);
    for second in 0..MANY {
        zfs = zfs.snapshot(&format!("tank/ROOT/be@s{second:05}"), 1_000 + second);
    }
    zfs
}

fn dataset(name: &str) -> DatasetName {
    name.parse().unwrap()
}

#[test]
fn start_up_keeps_only_the_count_and_the_newest_of_ten_thousand_snapshots() {
    let zfs = machine();

    let found = environments(&zfs, None).unwrap();

    let [environment] = found.as_slice() else {
        panic!("{found:?}");
    };
    assert_eq!(environment.snapshot_count, 10_000);
    let newest = environment.newest_snapshot.as_ref().unwrap();
    assert_eq!(
        (newest.name.name(), newest.created),
        ("s09999", 1_000 + MANY - 1)
    );
    // One listing of the environment's own snapshots, and no other.
    let listings: Vec<String> = zfs
        .calls()
        .into_iter()
        .filter(|call| call.contains("snapshots"))
        .collect();
    assert_eq!(listings, ["own_snapshots tank/ROOT/be"]);
}

#[test]
fn a_load_returns_all_ten_thousand_snapshots_newest_first() {
    let zfs = machine();

    let list = snapshot_list(&zfs, &dataset("tank/ROOT/be"), LOADED_LENGTH).unwrap();

    assert_eq!(list.environment, dataset("tank/ROOT/be"));
    assert_eq!(list.snapshots.len(), 10_000);
    assert!(
        list.snapshots
            .windows(2)
            .all(|pair| pair[0].created > pair[1].created)
    );
    assert_eq!(list.snapshots[0].name.name(), "s09999");
    assert_eq!(list.snapshots[9_999].name.name(), "s00000");
    assert!(
        list.snapshots
            .iter()
            .all(|s| s.name.dataset().as_str() == "tank/ROOT/be")
    );
}

#[test]
fn a_list_longer_than_a_message_allows_is_not_loaded_and_says_why() {
    let zfs = machine();
    let list = snapshot_list(&zfs, &dataset("tank/ROOT/be"), LOADED_LENGTH).unwrap();
    let length = serde_json::to_vec(&list).unwrap().len();

    assert_eq!(encoded_length(&list), length);
    let too_long = snapshot_list(&zfs, &dataset("tank/ROOT/be"), length - 1).unwrap_err();
    assert_eq!(
        too_long,
        format!(
            "its 10000 snapshots take {length} bytes, more than the {} any menu is sent of one \
             environment",
            length - 1
        )
    );
    assert!(snapshot_list(&zfs, &dataset("tank/ROOT/be"), length).is_ok());
    let missing = snapshot_list(&zfs, &dataset("tank/ROOT/gone"), LOADED_LENGTH);
    assert_eq!(missing.unwrap_err(), "tank/ROOT/gone does not exist");
    assert_eq!(
        snapshot_summary(&zfs, &dataset("tank/ROOT/gone")),
        (0, None)
    );
}
