//! A forced import of a pool that another system has imported now.

use super::*;

/// What `zpool import` lists of `tank`, imported by another system now.
const ACTIVE: &str = "   pool: tank\n     id: 42\n  state: ONLINE\n status: The pool is \
    currently imported by another system.\n action: The pool must be exported from otherhost \
    (hostid=deadbeef) before it can be safely imported.\n config:\n\n\ttank        ONLINE\n\
    \t  sda       ONLINE\n";

#[test]
fn a_pool_in_use_elsewhere_now_is_not_forced() {
    let fake = importable(None);
    fake.runner.respond("zpool import", 0, ACTIVE);
    let force = options(Access::ReadOnly, ImportPolicy::Force, false);

    let why = import_tank(&fake, force).unwrap_err();

    assert_eq!(why, ["tank is imported by another system now"]);
    assert!(moves(&fake).is_empty(), "{:?}", moves(&fake));
    // Another pool of that name, by its identifier, is not that one.
    let other = ImportOptions {
        id: Some(7),
        ..force
    };
    assert_eq!(import_tank(&fake, other), Ok(()));
}

#[test]
fn only_a_forced_import_scans_for_it() {
    let fake = importable(None);
    fake.runner.respond("zpool import", 0, ACTIVE);
    let strict = options(Access::ReadOnly, ImportPolicy::Strict, false);

    // Without `-f`, `zpool import` refuses such a pool itself: no scan.
    assert_eq!(import_tank(&fake, strict), Ok(()));
    assert!(!fake.runner.lines().contains(&"zpool import".to_owned()));
}
