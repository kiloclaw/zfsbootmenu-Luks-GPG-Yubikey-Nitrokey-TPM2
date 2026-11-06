//! A new snapshot whose name is taken: the menu knows only the snapshots it
//! read (ADR 0016, point 6), so the name of one it never read reaches
//! `zfs`, which refuses it, saying why.

use super::*;

#[test]
fn a_taken_snapshot_name_fails_with_the_message_of_zfs() {
    let zfs = machine();

    let events = perform(
        &zfs,
        &mut FakeNeeds::default(),
        &Action::CreateSnapshot(snapshot("zroot/ROOT/void@first")),
    );

    let Outcome::Failed { why, .. } = outcome(&events) else {
        panic!("{events:?}");
    };
    let said = why.last().map(String::as_str).unwrap_or_default();
    assert!(
        said.ends_with("cannot create snapshot 'zroot/ROOT/void@first': dataset already exists"),
        "{said}"
    );
}
