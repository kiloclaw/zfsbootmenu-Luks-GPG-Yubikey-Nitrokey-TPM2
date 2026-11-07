//! A rollback that a clone or a hold of a newer snapshot stops, refused
//! before the pool is imported read/write for it.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn a_refused_rollback_imports_nothing_and_destroys_nothing() {
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
            &Action::Rollback(snapshot("zroot/ROOT/void@first")),
        );

        let Outcome::Failed {
            what, why: causes, ..
        } = outcome(&events)
        else {
            panic!("rolled back past a clone or a hold: {events:?}");
        };
        assert_eq!(what, "unable to roll zroot/ROOT/void back to first");
        assert_eq!(causes, &[why]);
        // Neither imported read/write nor unlocked: only the kernels looked at.
        assert_eq!(needs.asked, ["has_kernels zroot/ROOT/void@first"]);
        assert!(!zfs.calls().iter().any(|call| call.starts_with("rollback")));
        assert_eq!(zfs.snapshots(&dataset("zroot/ROOT/void")).unwrap().len(), 2);
    }
}

#[test]
fn a_rollback_whose_newer_snapshots_cannot_be_listed_is_left_to_zfs() {
    let zfs = machine().failing("snapshot_details", "out of memory");
    let mut needs = FakeNeeds::default();

    let events = perform(
        &zfs,
        &mut needs,
        &Action::Rollback(snapshot("zroot/ROOT/void@first")),
    );

    assert!(is_done(&events), "{events:?}");
    assert_eq!(zfs.snapshots(&dataset("zroot/ROOT/void")).unwrap().len(), 1);
}
