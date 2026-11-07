//! Destroying snapshots of a boot environment: all of them in one `zfs
//! destroy`, or none, refused before the pool is imported read/write when
//! a clone or a hold keeps one, when they are of several filesystems, and
//! when their filesystem is not a boot environment.

use pretty_assertions::assert_eq;

use super::*;

fn destroy(names: &[&str]) -> Action {
    Action::Destroy(names.iter().map(|name| snapshot(name)).collect())
}

/// Fails unless `events` end in a failure of `what` because of `why`.
fn assert_failed(events: &[Event], what: &str, why: &[&str]) {
    let Outcome::Failed {
        what: failed,
        why: causes,
        ..
    } = outcome(events)
    else {
        panic!("not refused: {events:?}");
    };
    assert_eq!(failed, what);
    assert_eq!(causes, why);
}

#[test]
fn a_destroy_destroys_every_snapshot_named_in_one_command() {
    let zfs = machine().snapshot("zroot/ROOT/void@third", 300);
    let mut needs = FakeNeeds::default();

    let events = perform(
        &zfs,
        &mut needs,
        &destroy(&[
            "zroot/ROOT/void@first",
            "zroot/ROOT/void@third",
            "zroot/ROOT/void@first",
        ]),
    );

    assert_eq!(
        outcome(&events),
        &Outcome::Done("zroot/ROOT/void@first,third destroyed".to_owned())
    );
    let left = zfs.snapshots(&dataset("zroot/ROOT/void")).unwrap();
    let names: Vec<String> = left.iter().map(|left| left.name.to_string()).collect();
    assert_eq!(names, ["zroot/ROOT/void@second"]);
    // The pool imported read/write; no key is needed to destroy.
    assert_eq!(needs.asked, ["writable zroot"]);
    let calls = zfs.calls();
    let destroys = calls
        .iter()
        .filter(|call| call.starts_with("destroy_snapshots"))
        .count();
    assert_eq!(destroys, 1, "{calls:?}");
}

#[test]
fn a_clone_or_a_hold_refuses_the_destroy_before_the_pool_is_written() {
    for (held, why) in [
        (
            false,
            "zroot/ROOT/void@second has the clone zroot/ROOT/copy",
        ),
        (true, "zroot/ROOT/void@second is held by backup"),
    ] {
        let zfs = if held {
            machine().hold("zroot/ROOT/void@second", "backup")
        } else {
            machine()
                .filesystem("zroot/ROOT/copy", &[])
                .origin("zroot/ROOT/copy", "zroot/ROOT/void@second")
        };
        let mut needs = FakeNeeds::default();

        let events = perform(
            &zfs,
            &mut needs,
            &destroy(&["zroot/ROOT/void@first", "zroot/ROOT/void@second"]),
        );

        assert_failed(
            &events,
            "unable to destroy 2 snapshots of zroot/ROOT/void",
            &[why],
        );
        assert!(needs.asked.is_empty(), "{:?}", needs.asked);
        assert!(
            !zfs.calls()
                .iter()
                .any(|call| call.starts_with("destroy_snapshots"))
        );
        assert_eq!(zfs.snapshots(&dataset("zroot/ROOT/void")).unwrap().len(), 2);
    }
}

#[test]
fn what_is_not_one_boot_environment_s_snapshots_is_not_destroyed() {
    let zfs = machine()
        .filesystem("zroot/data", &[("mountpoint", "/data")])
        .snapshot("zroot/data@first", 100);
    for (action, what, why) in [
        (
            destroy(&[]),
            "unable to destroy snapshots",
            "no snapshot was chosen",
        ),
        (
            destroy(&["zroot/ROOT/void@first", "zroot/data@first"]),
            "unable to destroy 2 snapshots of zroot/ROOT/void",
            "zroot/data@first is not of zroot/ROOT/void",
        ),
        (
            destroy(&["zroot/data@first"]),
            "unable to destroy a snapshot of zroot/data",
            "zroot/data is not a boot environment",
        ),
    ] {
        let mut needs = FakeNeeds::default();

        let events = perform(&zfs, &mut needs, &action);

        assert_failed(&events, what, &[why]);
        assert!(needs.asked.is_empty(), "{:?}", needs.asked);
    }
    assert!(
        !zfs.calls()
            .iter()
            .any(|call| call.starts_with("destroy_snapshots"))
    );
}

#[test]
fn a_destroy_fails_with_the_pool_read_only_or_the_message_of_zfs() {
    let zfs = machine();
    let mut needs = FakeNeeds {
        read_only: true,
        ..FakeNeeds::default()
    };

    let events = perform(&zfs, &mut needs, &destroy(&["zroot/ROOT/void@first"]));

    assert_failed(
        &events,
        "unable to destroy a snapshot of zroot/ROOT/void",
        &["zroot may not be imported read/write"],
    );

    let zfs = machine().failing("destroy_snapshots", "pool I/O is currently suspended");
    let events = perform(
        &zfs,
        &mut FakeNeeds::default(),
        &destroy(&["zroot/ROOT/void@first"]),
    );

    let Outcome::Failed { what, why, .. } = outcome(&events) else {
        panic!("{events:?}");
    };
    assert_eq!(what, "unable to destroy a snapshot of zroot/ROOT/void");
    assert!(
        why.iter()
            .any(|why| why.contains("pool I/O is currently suspended")),
        "{why:?}"
    );
}
