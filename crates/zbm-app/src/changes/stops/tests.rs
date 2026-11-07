//! What stops a destroy, on [`FakeZfs`].

use pretty_assertions::assert_eq;
use zbm_testkit::zfs::FakeZfs;
use zbm_types::Untrusted;

use super::*;
use crate::model::PoolHost;

fn snapshot(text: &str) -> SnapshotName {
    text.parse().unwrap()
}

/// A boot environment with three snapshots, two taken in one second, and a
/// bookmark of the newest.
fn machine() -> FakeZfs {
    FakeZfs::new()
        .pool("zroot")
        .filesystem("zroot/ROOT/void", &[])
        .snapshot("zroot/ROOT/void@first", 100)
        .snapshot("zroot/ROOT/void@b", 200)
        .snapshot("zroot/ROOT/void@a", 200)
        .bookmark("zroot/ROOT/void@b", "zroot/ROOT/void#b")
}

#[test]
fn newer_is_what_zfs_took_after_even_in_the_same_second() {
    let zfs = machine();
    let details = zfs
        .snapshot_details(&"zroot/ROOT/void".parse().unwrap())
        .unwrap();

    let found = newer(&details, &snapshot("zroot/ROOT/void@a")).unwrap();

    let names: Vec<String> = found.iter().map(|detail| detail.name.to_string()).collect();
    assert_eq!(names, ["zroot/ROOT/void@b", "zroot/ROOT/void#b"]);
    assert_eq!(newer(&details, &snapshot("zroot/ROOT/void@gone")), None);
}

#[test]
fn clones_and_holds_of_newer_snapshots_stop_a_rollback() {
    let zfs = machine()
        .filesystem("zroot/ROOT/copy", &[])
        .origin("zroot/ROOT/copy", "zroot/ROOT/void@a")
        .hold("zroot/ROOT/void@b", "backup")
        .hold("zroot/ROOT/void@b", "send");

    let stops = of_rollback(&zfs, &snapshot("zroot/ROOT/void@first"));

    let b = snapshot("zroot/ROOT/void@b");
    assert_eq!(
        stops,
        [
            Stop::Clone {
                snapshot: snapshot("zroot/ROOT/void@a"),
                clone: Untrusted::new("zroot/ROOT/copy".to_owned()),
            },
            Stop::Hold {
                snapshot: b.clone(),
                tag: Some(Untrusted::new("backup".to_owned())),
            },
            Stop::Hold {
                snapshot: b,
                tag: Some(Untrusted::new("send".to_owned())),
            },
        ]
    );
    let words: Vec<String> = stops.iter().map(why).collect();
    assert_eq!(
        words,
        [
            "zroot/ROOT/void@a has the clone zroot/ROOT/copy",
            "zroot/ROOT/void@b is held by backup",
            "zroot/ROOT/void@b is held by send",
        ]
    );
    // What is older than the snapshot rolled back to stops nothing.
    assert!(of_rollback(&zfs, &snapshot("zroot/ROOT/void@b")).is_empty());
}

#[test]
fn a_hold_whose_tags_cannot_be_read_still_stops() {
    let zfs = machine()
        .hold("zroot/ROOT/void@b", "backup")
        .failing("holds", "no permission");

    let stops = of_rollback(&zfs, &snapshot("zroot/ROOT/void@first"));

    assert_eq!(
        stops,
        [Stop::Hold {
            snapshot: snapshot("zroot/ROOT/void@b"),
            tag: None,
        }]
    );
    assert_eq!(why(&stops[0]), "zroot/ROOT/void@b is held");
}

#[test]
fn a_listing_that_fails_stops_nothing() {
    let zfs = machine()
        .filesystem("zroot/ROOT/copy", &[])
        .origin("zroot/ROOT/copy", "zroot/ROOT/void@a")
        .failing("snapshot_details", "out of memory");

    assert!(of_rollback(&zfs, &snapshot("zroot/ROOT/void@first")).is_empty());
}

#[test]
fn an_active_pool_is_named_with_its_host_when_known() {
    let pool: zbm_types::PoolName = "tank".parse().unwrap();
    let unknown = Stop::Active {
        pool: pool.clone(),
        host: None,
    };
    let named = Stop::Active {
        pool,
        host: Some(PoolHost {
            hostid: None,
            hostname: Some(Untrusted::new("other\u{1b}[2J".to_owned())),
        }),
    };

    assert_eq!(why(&unknown), "tank is imported by another system now");
    let words = why(&named);
    assert!(words.starts_with("tank is imported by other"), "{words}");
    assert!(!words.contains('\u{1b}'), "{words}");
}
