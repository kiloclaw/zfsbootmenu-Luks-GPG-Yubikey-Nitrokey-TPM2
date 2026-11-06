//! The export as a change: what it asks of ZFS, and how it ends.

use pretty_assertions::assert_eq;

use super::*;

fn export(pool: &str) -> Action {
    Action::Export(pool.parse().unwrap())
}

#[test]
fn an_export_runs_zpool_export_and_lists_the_pool_to_import_again() {
    let zfs = machine();
    let mut needs = FakeNeeds::default();

    let events = perform(&zfs, &mut needs, &export("zroot"));

    assert_eq!(
        outcome(&events),
        &Outcome::Done("zroot is exported".to_owned())
    );
    assert!(zfs.pools().unwrap().is_empty());
    let found: Vec<String> = zfs
        .importable()
        .unwrap()
        .iter()
        .map(|pool| pool.name.to_string())
        .collect();
    assert_eq!(found, ["zroot"]);
    // Nothing is imported read/write, no key loaded, nothing mounted.
    assert_eq!(needs.asked, Vec::<String>::new());
}

#[test]
fn a_busy_pool_fails_the_export_with_its_cause() {
    let zfs = machine().busy("zroot");
    let mut needs = FakeNeeds::default();

    let events = perform(&zfs, &mut needs, &export("zroot"));

    let Outcome::Failed { what, why, .. } = outcome(&events) else {
        panic!("{events:?}");
    };
    assert_eq!(what, "unable to export zroot");
    assert!(why[0].contains("pool is busy"), "{why:?}");
    assert_eq!(zfs.pools().unwrap().len(), 1);
}

#[test]
fn an_export_is_a_change_the_lock_keeps_that_needs_no_writable_pool() {
    let action = export("zroot");
    let bootfs: DatasetName = "zroot/ROOT/void".parse().unwrap();

    assert!(action.is_change() && !action.needs_writable_pool());
    assert!(!action.needs_console() && !action.is_read());
    assert_eq!(action.changed_environment(), None);
    // The pool of the default environment too: no refusal.
    assert!(action.is_kept_by_lock(Some(&bootfs)));
    assert!(!action.needs_console_on(&crate::model::Snapshot::default()));
    let locked = crate::model::Snapshot {
        menu_locked: true,
        ..crate::model::Snapshot::default()
    };
    // The PIN is asked on the console.
    assert!(action.needs_console_on(&locked));
    assert_eq!(action.offer(), Some(crate::model::Offer::Export));
}
