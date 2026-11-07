//! Tests of the snapshots and bookmarks of [`FakeZfs`] and what keeps
//! them.

use pretty_assertions::assert_eq;
use zbm_types::PoolName;
use zbm_zfs::Zfs;

use super::*;

fn machine() -> FakeZfs {
    FakeZfs::new()
        .pool("zroot")
        .filesystem("zroot/ROOT/void", &[])
        .filesystem("zroot/home", &[])
        .snapshot("zroot/ROOT/void@first", 100)
        .snapshot("zroot/ROOT/void@b", 200)
        .snapshot("zroot/ROOT/void@a", 200)
        .snapshot("zroot/home@x", 150)
}

fn dataset(text: &str) -> DatasetName {
    text.parse().unwrap()
}

fn snapshot(text: &str) -> SnapshotName {
    text.parse().unwrap()
}

/// The snapshots and bookmarks of `of`, by their names after the dataset.
fn names(zfs: &FakeZfs, of: &str) -> Vec<String> {
    zfs.snapshot_details(&dataset(of))
        .unwrap()
        .iter()
        .map(|detail| detail.name.to_string().split_off(of.len()))
        .collect()
}

/// The `createtxg` of each snapshot and bookmark of `of`, by name.
fn txgs(zfs: &FakeZfs, of: &str) -> Vec<(String, u64)> {
    zfs.snapshot_details(&dataset(of))
        .unwrap()
        .iter()
        .map(|detail| {
            let name = detail.name.to_string().split_off(of.len());
            (name, detail.createtxg)
        })
        .collect()
}

/// What `zfs` says of `snapshot`, held.
fn busy(snapshot: &str) -> String {
    format!(
        "cannot destroy snapshot {snapshot}: it's being held. \
         Run 'zfs holds -r {snapshot}' to see holders."
    )
}

fn failure(result: Result<(), ZfsError>) -> String {
    match result {
        Err(ZfsError::Failed { message, .. }) => message,
        other => panic!("{other:?}"),
    }
}

/// A detail as a test compares it: name, createtxg, used, userrefs and
/// the number of clones.
type Row = (String, u64, Option<u64>, u64, usize);

#[test]
fn snapshots_taken_in_one_second_are_in_the_order_of_their_names() {
    let zfs = machine()
        .bookmark("zroot/ROOT/void@first", "zroot/ROOT/void#first")
        .hold("zroot/ROOT/void@a", "keep")
        .hold("zroot/ROOT/void@a", "send")
        .origin("zroot/home", "zroot/ROOT/void@b");

    let details = zfs.snapshot_details(&dataset("zroot/ROOT/void")).unwrap();

    let shown: Vec<Row> = details
        .iter()
        .map(|detail| {
            (
                detail.name.to_string(),
                detail.createtxg,
                detail.used,
                detail.userrefs,
                detail.clones.len(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("zroot/ROOT/void@first".to_owned(), 1, Some(4096), 0, 0),
            ("zroot/ROOT/void#first".to_owned(), 1, None, 0, 0),
            ("zroot/ROOT/void@a".to_owned(), 3, Some(4096), 2, 0),
            ("zroot/ROOT/void@b".to_owned(), 4, Some(4096), 0, 1),
        ]
    );
    assert_eq!(details[3].clones[0].get(), "zroot/home");
    assert!(matches!(
        zfs.snapshot_details(&dataset("zroot/none")),
        Err(ZfsError::Missing(_))
    ));
    assert_eq!(zfs.calls()[0], "snapshot_details zroot/ROOT/void");
}

#[test]
fn holds_are_listed_for_the_snapshots_asked() {
    let zfs = machine()
        .hold("zroot/ROOT/void@a", "keep")
        .hold("zroot/home@x", "backup");

    let holds = zfs
        .holds(&[snapshot("zroot/ROOT/void@a"), snapshot("zroot/ROOT/void@b")])
        .unwrap();

    assert_eq!(holds.len(), 1);
    assert_eq!(holds[0].snapshot, snapshot("zroot/ROOT/void@a"));
    assert_eq!(holds[0].tag.get(), "keep");
    assert!(zfs.holds(&[]).unwrap().is_empty());
    let missing = zfs.holds(&[
        snapshot("zroot/ROOT/void@none"),
        snapshot("zroot/ROOT/void@a"),
    ]);
    assert_eq!(
        failure(missing.map(drop)),
        "cannot open 'zroot/ROOT/void@none': dataset does not exist"
    );
    let failing = machine().failing("holds", "no holds");
    assert_eq!(failure(failing.holds(&[]).map(drop)), "no holds");
    assert_eq!(zfs.calls()[0], "holds zroot/ROOT/void@a,zroot/ROOT/void@b");
}

#[test]
fn a_destroy_count_frees_each_snapshot_named_and_refuses_a_clone() {
    let zfs = machine().hold("zroot/ROOT/void@a", "keep");
    let each = |names: &[&str]| SnapshotSet::Each(names.iter().map(|n| snapshot(n)).collect());
    let range = SnapshotSet::Range {
        first: snapshot("zroot/ROOT/void@first"),
        last: snapshot("zroot/ROOT/void@a"),
    };

    // A hold does not stop the count, which destroys nothing.
    assert_eq!(
        zfs.destroy_space(&each(&["zroot/ROOT/void@a"])).unwrap(),
        4096
    );
    assert_eq!(zfs.destroy_space(&range).unwrap(), 8192);
    let space = |set: &SnapshotSet| failure(zfs.destroy_space(set).map(drop));
    assert_eq!(space(&each(&[])), "missing dataset argument");
    assert_eq!(
        space(&each(&["zroot/ROOT/void@a", "zroot/home@x"])),
        "too many arguments"
    );
    assert!(space(&each(&["zroot/ROOT/void@none"])).contains("could not find"));
    // A name that does not exist is passed over, as `zfs` does.
    assert_eq!(
        zfs.destroy_space(&each(&["zroot/ROOT/void@b", "zroot/ROOT/void@none"]))
            .unwrap(),
        4096
    );
    let cloned = machine().origin("zroot/home", "zroot/ROOT/void@b");
    let refused = cloned.destroy_space(&each(&["zroot/ROOT/void@b"]));
    assert!(failure(refused.map(drop)).contains("dependent clones"));
    let failing = machine().failing("destroy_space", "no count");
    assert_eq!(failure(failing.destroy_space(&range).map(drop)), "no count");
    assert_eq!(
        zfs.calls()[..2],
        [
            "destroy_space zroot/ROOT/void@a",
            "destroy_space zroot/ROOT/void@first%a",
        ]
    );
}

#[test]
fn a_hold_or_a_clone_keeps_every_snapshot_a_destroy_names() {
    let zfs = machine().hold("zroot/ROOT/void@a", "keep");

    let held =
        zfs.destroy_snapshots(&[snapshot("zroot/ROOT/void@b"), snapshot("zroot/ROOT/void@a")]);
    assert_eq!(failure(held), busy("zroot/ROOT/void@a"));
    assert_eq!(names(&zfs, "zroot/ROOT/void"), ["@first", "@a", "@b"]);
    zfs.destroy_snapshots(&[
        snapshot("zroot/ROOT/void@first"),
        snapshot("zroot/ROOT/void@b"),
    ])
    .unwrap();

    assert_eq!(names(&zfs, "zroot/ROOT/void"), ["@a"]);
    assert_eq!(
        failure(zfs.destroy_snapshots(&[])),
        "missing dataset argument"
    );
    let cloned = machine().origin("zroot/home", "zroot/ROOT/void@b");
    let refused = cloned.destroy_snapshots(&[snapshot("zroot/ROOT/void@b")]);
    assert!(failure(refused).contains("dependent clones"));
    let read_only = machine().read_only("zroot");
    let refused = read_only.destroy_snapshots(&[snapshot("zroot/ROOT/void@b")]);
    assert_eq!(failure(refused), "pool is read-only");
    assert_eq!(zfs.calls()[0], "destroy_snapshots zroot/ROOT/void@b,a");
}

#[test]
fn a_destroy_passes_over_a_name_that_does_not_exist() {
    let zfs = machine();

    zfs.destroy_snapshots(&[
        snapshot("zroot/ROOT/void@b"),
        snapshot("zroot/ROOT/void@gone"),
    ])
    .unwrap();
    let none = zfs.destroy_snapshots(&[snapshot("zroot/ROOT/void@gone")]);

    assert_eq!(names(&zfs, "zroot/ROOT/void"), ["@first", "@a"]);
    assert_eq!(
        failure(none),
        "could not find any snapshots to destroy; check snapshot names."
    );
}

#[test]
fn a_range_runs_from_its_first_to_its_last_in_the_order_zfs_took_them() {
    let zfs = machine();
    let range = |first: &str, last: &str| SnapshotSet::Range {
        first: snapshot(&format!("zroot/ROOT/void@{first}")),
        last: snapshot(&format!("zroot/ROOT/void@{last}")),
    };
    let space = |set: &SnapshotSet| zfs.destroy_space(set).map_err(|e| failure(Err(e)));
    let nothing = Err("could not find any snapshots to destroy; check snapshot names.".to_owned());

    // `a` and `b` were taken in one second, `a` first.
    assert_eq!(space(&range("first", "b")), Ok(3 * 4096));
    assert_eq!(space(&range("a", "a")), Ok(4096));
    assert_eq!(space(&range("b", "a")), nothing);
    assert_eq!(space(&range("first", "gone")), nothing);
    assert_eq!(space(&range("gone", "b")), nothing);
}

#[test]
fn a_rollback_destroys_what_zfs_took_later() {
    let zfs = machine()
        .bookmark("zroot/ROOT/void@b", "zroot/ROOT/void#b")
        .bookmark("zroot/ROOT/void@first", "zroot/ROOT/void#first");

    // `b` was taken in the same second as `a`, after it.
    zfs.rollback(&snapshot("zroot/ROOT/void@a")).unwrap();

    assert_eq!(names(&zfs, "zroot/ROOT/void"), ["@first", "#first", "@a"]);
}

#[test]
fn a_hold_stops_a_rollback_only_after_the_rest_is_destroyed() {
    let zfs = machine()
        .snapshot("zroot/ROOT/void@c", 300)
        .hold("zroot/ROOT/void@a", "keep")
        .bookmark("zroot/ROOT/void@b", "zroot/ROOT/void#b")
        .bookmark("zroot/ROOT/void@first", "zroot/ROOT/void#first");

    let refused = zfs.rollback(&snapshot("zroot/ROOT/void@first"));

    assert_eq!(failure(refused), busy("zroot/ROOT/void@a"));
    // As `zfs rollback -r`: `b`, `c` and the bookmark `b` are gone, and the
    // filesystem was not rolled back.
    assert_eq!(names(&zfs, "zroot/ROOT/void"), ["@first", "#first", "@a"]);
}

#[test]
fn a_clone_stops_a_rollback_before_anything_is_destroyed() {
    let zfs = machine()
        .hold("zroot/ROOT/void@a", "keep")
        .bookmark("zroot/ROOT/void@b", "zroot/ROOT/void#b")
        .origin("zroot/home", "zroot/ROOT/void@b");

    let refused = zfs.rollback(&snapshot("zroot/ROOT/void@first"));

    assert!(failure(refused).contains("dependent clones"));
    assert_eq!(names(&zfs, "zroot/ROOT/void"), ["@first", "@a", "@b", "#b"]);
}

#[test]
fn a_createtxg_never_changes_and_a_new_snapshot_comes_last() {
    let zfs = machine()
        .snapshot("zroot/ROOT/void@c", 300)
        .bookmark("zroot/ROOT/void@a", "zroot/ROOT/void#a");
    let before = txgs(&zfs, "zroot/ROOT/void");

    zfs.destroy_snapshots(&[snapshot("zroot/home@x")]).unwrap();
    zfs.destroy_snapshots(&[snapshot("zroot/ROOT/void@a")])
        .unwrap();
    zfs.create_snapshot(&snapshot("zroot/ROOT/void@0")).unwrap();
    let after = txgs(&zfs, "zroot/ROOT/void");

    let kept: Vec<(String, u64)> = before
        .into_iter()
        .filter(|(name, _)| name != "@a")
        .collect();
    assert_eq!(after[..kept.len()], kept);
    assert_eq!(after[kept.len()], ("@0".to_owned(), 6));
    let late = zfs.snapshot("zroot/ROOT/void@late", 1);
    assert_eq!(txgs(&late, "zroot/ROOT/void").last().unwrap().1, 7);
}

#[test]
fn holds_and_bookmarks_go_with_their_pool_and_holds_move_with_a_promotion() {
    let zfs = machine()
        .hold("zroot/ROOT/void@first", "keep")
        .bookmark("zroot/ROOT/void@first", "zroot/ROOT/void#first")
        .origin("zroot/home", "zroot/ROOT/void@first");

    zfs.promote(&dataset("zroot/home")).unwrap();

    let moved = zfs.holds(&[snapshot("zroot/home@first")]).unwrap();
    assert_eq!(moved[0].tag.get(), "keep");
    let pool: PoolName = "zroot".parse().unwrap();
    zfs.export(&pool).unwrap();
    let state = zfs.state.borrow();
    assert!(state.holds.is_empty() && state.bookmarks.is_empty());
}

#[test]
#[should_panic(expected = "FakeZfs has no snapshot")]
fn a_bookmark_of_a_missing_snapshot_is_a_mistake() {
    let _ = machine().bookmark("zroot/ROOT/void@none", "zroot/ROOT/void#none");
}

#[test]
fn a_listing_planned_to_fail_fails() {
    let zfs = FakeZfs::new()
        .pool("zroot")
        .filesystem("zroot/ROOT/void", &[])
        .failing("snapshot_details", "out of memory");

    let error = zfs
        .snapshot_details(&dataset("zroot/ROOT/void"))
        .unwrap_err();

    assert!(error.to_string().contains("out of memory"), "{error}");
}
