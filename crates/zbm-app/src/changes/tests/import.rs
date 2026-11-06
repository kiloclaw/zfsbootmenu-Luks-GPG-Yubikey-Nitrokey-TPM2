//! The import as a change: what it asks of the machine, and how it ends.

use pretty_assertions::assert_eq;

use super::*;

fn import(access: Access, rewind: bool) -> Action {
    let options = ImportOptions {
        access,
        policy: zbm_types::ImportPolicy::Strict,
        rewind,
        id: None,
    };
    Action::Import("tank".parse().unwrap(), options)
}

#[test]
fn an_import_asks_the_machine_and_says_how_the_pool_is_imported() {
    let zfs = machine();
    let read_write = "tank is rewound to its checkpoint and imported read/write";
    for (access, rewind, said) in [
        (Access::ReadOnly, false, "tank is imported read-only"),
        (Access::ReadWrite, false, "tank is imported read/write"),
        (Access::ReadWrite, true, read_write),
    ] {
        let mut needs = FakeNeeds::default();
        let events = perform(&zfs, &mut needs, &import(access, rewind));
        assert_eq!(outcome(&events), &Outcome::Done(said.to_owned()));
        let options = ImportOptions {
            access,
            policy: zbm_types::ImportPolicy::Strict,
            rewind,
            id: None,
        };
        assert_eq!(needs.asked, [format!("import tank {options:?}")]);
    }
}

#[test]
fn an_import_the_machine_refuses_fails_with_its_reasons() {
    let zfs = machine();
    let mut needs = FakeNeeds {
        import_error: Some("tank is degraded".to_owned()),
        ..FakeNeeds::default()
    };

    let events = perform(&zfs, &mut needs, &import(Access::ReadWrite, true));

    assert_eq!(
        outcome(&events),
        &Outcome::Failed {
            what: "unable to rewind tank to its checkpoint".to_owned(),
            why: vec!["tank is degraded".to_owned()],
            next: Vec::new(),
        }
    );
}

#[test]
fn an_import_takes_the_console_and_needs_no_writable_pool_first() {
    let read_only = import(Access::ReadOnly, false);
    assert!(read_only.is_change() && read_only.needs_console());
    assert!(!read_only.needs_writable_pool());
    assert_eq!(read_only.changed_environment(), None);
    assert!(read_only.needs_console_on(&crate::model::Snapshot::default()));
}
