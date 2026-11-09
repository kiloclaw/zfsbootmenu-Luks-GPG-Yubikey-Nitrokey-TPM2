//! The reviews of a rollback, a rewind and a forced import, on
//! [`FakeZfs`].

use pretty_assertions::assert_eq;
use zbm_testkit::zfs::FakeZfs;
use zbm_types::Untrusted;

use super::*;

fn snapshot(text: &str) -> SnapshotName {
    text.parse().unwrap()
}

fn pool(text: &str) -> PoolName {
    text.parse().unwrap()
}

/// A boot environment whose snapshots `@b` and `@a` were taken in one
/// second, after `@first`; `@b` has a bookmark, and 12345 bytes were
/// written since `@first`.
fn machine() -> FakeZfs {
    FakeZfs::new()
        .pool("zroot")
        .filesystem("zroot/ROOT/void", &[("written@first", "12345")])
        .snapshot("zroot/ROOT/void@old", 50)
        .snapshot("zroot/ROOT/void@first", 100)
        .snapshot("zroot/ROOT/void@b", 200)
        .snapshot("zroot/ROOT/void@a", 200)
        .bookmark("zroot/ROOT/void@b", "zroot/ROOT/void#b")
        .bookmark("zroot/ROOT/void@old", "zroot/ROOT/void#old")
}

fn import(policy: ImportPolicy, rewind: bool, id: Option<u64>) -> ImportOptions {
    ImportOptions {
        access: Access::ReadWrite,
        policy,
        rewind,
        id,
    }
}

#[test]
fn a_rollback_loses_what_zfs_took_after_it_even_in_the_same_second() {
    let zfs = machine();
    let first = snapshot("zroot/ROOT/void@first");

    let review = review(&zfs, &Action::Rollback(first.clone()));

    assert_eq!(
        review,
        Review {
            loses: vec![
                Loss::Snapshot {
                    name: snapshot("zroot/ROOT/void@a"),
                    used: Some(4096),
                },
                Loss::Snapshot {
                    name: snapshot("zroot/ROOT/void@b"),
                    used: Some(4096),
                },
                Loss::Bookmark {
                    dataset: "zroot/ROOT/void".parse().unwrap(),
                    name: "b".to_owned(),
                },
                Loss::Written {
                    since: first.clone(),
                    bytes: Some(12_345),
                },
            ],
            freed: Some(8192),
            keeps: vec![Keep::Snapshots(1), Keep::Bookmarks(1)],
            undo: None,
            host_id_stays: false,
            safer: Some(Safer::Clone(first)),
            stops: Vec::new(),
            pool_use: None,
        }
    );
    assert!(
        zfs.calls()
            .contains(&"destroy_space zroot/ROOT/void@a%b".to_owned()),
        "{:?}",
        zfs.calls()
    );
}

#[test]
fn a_rollback_to_the_newest_snapshot_frees_nothing() {
    let zfs = machine();

    let review = review(&zfs, &Action::Rollback(snapshot("zroot/ROOT/void@b")));

    assert_eq!(review.freed, Some(0));
    // The bookmark of `@b` is no newer than it, and stays.
    assert_eq!(review.keeps, [Keep::Snapshots(3), Keep::Bookmarks(2)]);
    assert!(
        matches!(review.loses.as_slice(), [Loss::Written { bytes: None, .. }]),
        "{review:?}"
    );
}

#[test]
fn a_clone_or_a_hold_of_a_newer_snapshot_stops_a_rollback() {
    let zfs = machine()
        .filesystem("zroot/ROOT/copy", &[])
        .origin("zroot/ROOT/copy", "zroot/ROOT/void@a")
        .hold("zroot/ROOT/void@b", "backup");
    let rollback = Action::Rollback(snapshot("zroot/ROOT/void@first"));

    let review = review(&zfs, &rollback);

    assert_eq!(
        review.stops,
        [
            Stop::Clone {
                snapshot: snapshot("zroot/ROOT/void@a"),
                clone: Untrusted::new("zroot/ROOT/copy".to_owned()),
            },
            Stop::Hold {
                snapshot: snapshot("zroot/ROOT/void@b"),
                tag: Some(Untrusted::new("backup".to_owned())),
            },
        ]
    );
    // `zfs destroy -n` refuses a clone too: the space is not counted.
    assert_eq!(review.freed, None);
    let Outcome::Failed { why, .. } = refused(Outcome::Done(String::new()), &review) else {
        panic!("a plan with a stop is done");
    };
    assert_eq!(
        why,
        [
            "zroot/ROOT/void@a has the clone zroot/ROOT/copy",
            "zroot/ROOT/void@b is held by backup",
        ]
    );

    // A hold alone lets the space be counted.
    let held = machine().hold("zroot/ROOT/void@b", "backup");
    assert_eq!(review_of(&held, &rollback).freed, Some(8192));
}

/// The review of `action` on `zfs`, for a test whose own `review` hides
/// [`review`].
fn review_of(zfs: &FakeZfs, action: &Action) -> Review {
    review(zfs, action)
}

#[test]
fn reads_that_fail_leave_what_they_give_unknown() {
    let zfs = machine()
        .failing("snapshot_details", "out of memory")
        .failing("destroy_space", "out of memory");
    let first = snapshot("zroot/ROOT/void@first");

    let review = review(&zfs, &Action::Rollback(first.clone()));

    assert_eq!(
        review.loses,
        [
            Loss::Newer(first.clone()),
            Loss::Written {
                since: first.clone(),
                bytes: Some(12_345),
            },
        ]
    );
    assert_eq!((review.freed, review.keeps.len()), (None, 0));
    assert!(review.stops.is_empty());
    assert_eq!(review.safer, Some(Safer::Clone(first)));

    // The listing read, the space alone is unknown.
    let zfs = machine().failing("destroy_space", "out of memory");
    let review = review_of(&zfs, &Action::Rollback(snapshot("zroot/ROOT/void@first")));
    assert_eq!((review.freed, review.loses.len()), (None, 4));
    // A failed plan stays as it failed.
    let failed = Outcome::Failed {
        what: "x".to_owned(),
        why: Vec::new(),
        next: Vec::new(),
    };
    assert_eq!(refused(failed.clone(), &review), failed);
}

#[test]
fn a_rewind_loses_what_was_made_since_the_checkpoint() {
    // Taken at 1793966400 seconds.
    let zfs = FakeZfs::new()
        .pool("tank")
        .pool_setting("tank", "checkpoint", "1572864")
        .checkpoint_created("tank", "Fri Nov  6 12:00:00 2026")
        .filesystem("tank", &[("creation", "1700000000")])
        .filesystem("tank/old", &[("creation", "1793966399")])
        .filesystem("tank/new", &[("creation", "1793966400")])
        .snapshot("tank/old@before", 1_793_966_399)
        .snapshot("tank/old@after", 1_793_966_500);
    let options = import(ImportPolicy::Strict, true, None);

    let review = review(&zfs, &Action::Import(pool("tank"), options));

    assert_eq!(
        review.loses,
        [Loss::SinceCheckpoint {
            created: Some(Untrusted::new("Fri Nov  6 12:00:00 2026".to_owned())),
            space: Some(1_572_864),
            discarding: false,
            datasets: Some(vec!["tank/new".parse().unwrap()]),
            snapshots: Some(vec![snapshot("tank/old@after")]),
        }]
    );
    let read_only = ImportOptions {
        access: Access::ReadOnly,
        rewind: false,
        ..options
    };
    assert_eq!(
        review.safer,
        Some(Safer::Import(Box::new(Action::Import(
            pool("tank"),
            read_only
        ))))
    );
    assert_eq!(review.undo, None);
    // Imported here, the pool is this host's.
    assert_eq!(
        review.pool_use,
        Some(PoolUse {
            this_host: true,
            ..PoolUse::default()
        })
    );
    assert!(review.stops.is_empty());
}

#[test]
fn a_rewind_whose_checkpoint_time_is_unknown_lists_nothing_made_since() {
    let zfs = FakeZfs::new()
        .pool("tank")
        .pool_setting("tank", "checkpoint", "4096")
        .filesystem("tank", &[]);
    let rewind = Action::Import(pool("tank"), import(ImportPolicy::Strict, true, None));

    let review = review(&zfs, &rewind);

    let [
        Loss::SinceCheckpoint {
            created: None,
            space: Some(4096),
            datasets: None,
            snapshots: None,
            ..
        },
    ] = review.loses.as_slice()
    else {
        panic!("{review:?}");
    };

    // Nor when the checkpoint cannot be read.
    let zfs = FakeZfs::new().pool("tank").failing("checkpoint", "busy");
    let [Loss::SinceCheckpoint { space: None, .. }] = review_of(&zfs, &rewind).loses.as_slice()
    else {
        panic!("a checkpoint read");
    };
}

#[test]
fn a_forced_import_names_who_used_the_pool_last_and_when() {
    let zfs = FakeZfs::new().foreign("tank", 7, "deadbeef", "otherhost", 1_793_000_000);

    let review = review(
        &zfs,
        &Action::Import(pool("tank"), import(ImportPolicy::Force, false, None)),
    );

    let host = review.pool_use.as_ref().and_then(|used| used.host.as_ref());
    assert_eq!(
        host.and_then(|host| host.hostname.clone()),
        Some(Untrusted::new("otherhost".to_owned()))
    );
    assert_eq!(
        host.and_then(|host| host.hostid.as_ref())
            .map(ToString::to_string)
            .as_deref(),
        Some("deadbeef")
    );
    let used = review.pool_use.clone().unwrap();
    assert_eq!(
        (used.written, used.this_host, used.active),
        (Some(1_793_000_000), false, false)
    );
    assert_eq!(review.undo, Some(Box::new(Action::Export(pool("tank")))));
    assert_eq!((review.safer, review.stops.len()), (None, 0));
    assert!(review.loses.is_empty());
}

#[test]
fn a_forced_import_of_a_pool_in_use_elsewhere_now_is_stopped() {
    let zfs = FakeZfs::new()
        .foreign("tank", 7, "deadbeef", "otherhost", 1_793_000_000)
        .exported("tank", 8)
        .active("tank");

    // By its identifier, the pool listed with it.
    let by_id = Action::Import(pool("tank"), import(ImportPolicy::Force, false, Some(8)));
    let review = review(&zfs, &by_id);

    let [
        Stop::Active {
            pool: stopped,
            host,
        },
    ] = review.stops.as_slice()
    else {
        panic!("{review:?}");
    };
    assert_eq!(stopped, &pool("tank"));
    assert_eq!(host, &Some(PoolHost::default()));
    assert!(review.pool_use.is_some_and(|used| used.active));
    assert!(matches!(
        refused(Outcome::Done(String::new()), &review_of(&zfs, &by_id)),
        Outcome::Failed { .. }
    ));
}

#[test]
fn another_pool_of_the_name_imported_here_is_not_the_one_named() {
    // `tank` of identifier 1000 is imported here; the `tank` of 7 is in
    // use elsewhere, and has a checkpoint of its own.
    let zfs = FakeZfs::new()
        .pool("tank")
        .pool_setting("tank", "checkpoint", "4096")
        .foreign("tank", 7, "deadbeef", "otherhost", 1_793_000_000)
        .active("tank");

    for rewind in [false, true] {
        let options = import(ImportPolicy::Force, rewind, Some(7));
        let review = review(&zfs, &Action::Import(pool("tank"), options));

        let used = review.pool_use.clone().unwrap();
        assert_eq!(
            (used.this_host, used.active, used.written),
            (false, true, Some(1_793_000_000)),
            "{review:?}"
        );
        assert!(matches!(review.stops.as_slice(), [Stop::Active { .. }]));
        if rewind {
            // Not the checkpoint of the pool imported here.
            let [Loss::SinceCheckpoint { space: None, .. }] = review.loses.as_slice() else {
                panic!("{review:?}");
            };
        } else {
            assert_eq!(review.undo, Some(Box::new(Action::Export(pool("tank")))));
        }
    }

    // Named by its own identifier, the pool imported here is this host's.
    let options = import(ImportPolicy::Force, false, Some(1000));
    let review = review_of(&zfs, &Action::Import(pool("tank"), options));
    assert!(review.pool_use.is_some_and(|used| used.this_host));
    assert!(review.stops.is_empty() && review.undo.is_none());
}

#[test]
fn a_host_that_cannot_be_read_is_unknown_and_the_import_goes_on() {
    let zfs = FakeZfs::new()
        .foreign("tank", 7, "deadbeef", "otherhost", 1_793_000_000)
        .failing("pool_host", "zdb crashed");
    let forced = Action::Import(pool("tank"), import(ImportPolicy::Force, false, Some(7)));

    let review = review(&zfs, &forced);

    assert_eq!(
        review.pool_use,
        Some(PoolUse {
            host: None,
            written: None,
            this_host: false,
            active: false,
        })
    );
    assert!(review.stops.is_empty());

    // Nor does a failed scan stop it.
    let zfs = FakeZfs::new().failing("importable", "cancelled");
    let review = review_of(&zfs, &forced);
    assert!(review.stops.is_empty() && review.pool_use.is_some());
}

#[test]
fn an_action_that_needs_no_review_has_none() {
    let zfs = machine();
    let clone = Action::Clone(
        snapshot("zroot/ROOT/void@first"),
        "zroot/ROOT/copy".parse().unwrap(),
    );
    let export = Action::Export(pool("zroot"));

    assert_eq!(review(&zfs, &clone), Review::default());
    assert_eq!(review(&zfs, &export), Review::default());
}

#[test]
fn a_destroy_loses_its_snapshots_in_age_order_and_keeps_every_bookmark() {
    let zfs = machine();
    let b = snapshot("zroot/ROOT/void@b");
    let old = snapshot("zroot/ROOT/void@old");
    let gone = snapshot("zroot/ROOT/void@gone");

    let review = review(
        &zfs,
        &Action::Destroy(vec![b.clone(), gone.clone(), old.clone()]),
    );

    assert_eq!(
        review,
        Review {
            loses: vec![
                Loss::Snapshot {
                    name: old,
                    used: Some(4096),
                },
                Loss::Snapshot {
                    name: b,
                    used: Some(4096),
                },
                // Listed by none: no size.
                Loss::Snapshot {
                    name: gone,
                    used: None,
                },
            ],
            freed: Some(8192),
            keeps: vec![Keep::Bookmarks(2)],
            ..Review::default()
        }
    );
    assert!(
        zfs.calls()
            .contains(&"destroy_space zroot/ROOT/void@b,gone,old".to_owned()),
        "{:?}",
        zfs.calls()
    );
}

#[test]
fn a_destroy_whose_space_is_counted_says_what_it_frees() {
    let zfs = FakeZfs::new()
        .pool("zroot")
        .filesystem("zroot/ROOT/void", &[])
        .snapshot("zroot/ROOT/void@a", 100)
        .snapshot("zroot/ROOT/void@b", 200);

    let review = review(
        &zfs,
        &Action::Destroy(vec![
            snapshot("zroot/ROOT/void@a"),
            snapshot("zroot/ROOT/void@b"),
        ]),
    );

    assert_eq!(review.freed, Some(8192));
    assert!(review.keeps.is_empty());
    assert_eq!(review.undo, None);
    assert_eq!(review.safer, None);
}

#[test]
fn a_clone_or_a_hold_of_a_destroyed_snapshot_stops_it() {
    let zfs = machine()
        .filesystem("zroot/ROOT/copy", &[])
        .origin("zroot/ROOT/copy", "zroot/ROOT/void@a")
        .hold("zroot/ROOT/void@b", "backup");
    let a = snapshot("zroot/ROOT/void@a");
    let b = snapshot("zroot/ROOT/void@b");

    let review = review(&zfs, &Action::Destroy(vec![a.clone(), b.clone()]));

    assert_eq!(
        review.stops,
        [
            Stop::Clone {
                snapshot: a,
                clone: Untrusted::new("zroot/ROOT/copy".to_owned()),
            },
            Stop::Hold {
                snapshot: b,
                tag: Some(Untrusted::new("backup".to_owned())),
            },
        ]
    );
    // A clone: `zfs destroy -n` refuses too.
    assert_eq!(review.freed, None);
}

#[test]
fn a_destroy_whose_snapshots_cannot_be_listed_loses_them_with_no_size() {
    let zfs = machine()
        .failing("snapshot_details", "out of memory")
        .failing("destroy_space", "out of memory");
    let a = snapshot("zroot/ROOT/void@a");

    let lost = review(&zfs, &Action::Destroy(vec![a.clone()]));

    assert_eq!(
        lost,
        Review {
            loses: vec![Loss::Snapshot {
                name: a,
                used: None
            }],
            ..Review::default()
        }
    );
    assert_eq!(
        review(&zfs, &Action::Destroy(Vec::new())),
        Review::default()
    );
}
