//! Which changes take the console: those that write a pool imported
//! read-only, and those whose key may be asked for.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn a_change_takes_the_console_on_a_read_only_pool_or_a_locked_environment() {
    use crate::model::{CommandLine, Encryption, Environment, Health, Origin, Pool, Snapshot};
    let pool = |name: &str, writable| Pool {
        name: name.parse().unwrap(),
        health: Health::Online,
        writable,
        imported: true,
        id: None,
        host: None,
        checkpoint: Some(false),
        devices: Vec::new(),
        status: None,
        action: None,
        errors: None,
    };
    let environment = |name: &str, encryption| Environment {
        name: dataset(name),
        is_default: false,
        encryption,
        kernels: Vec::new(),
        command_line: CommandLine {
            value: Untrusted::default(),
            origin: Origin::Default,
        },
        snapshot_count: 0,
        newest_snapshot: None,
        checks: Vec::new(),
        tries: None,
    };
    // A pool that is not imported and is named like `zroot` is never
    // written: it leaves `zroot`'s changes quiet.
    let mut twin = pool("zroot", false);
    twin.imported = false;
    let machine = Snapshot {
        pools: vec![pool("zroot", true), pool("tank", false), twin],
        environments: vec![
            environment("zroot/ROOT/void", Encryption::None),
            environment("zroot/ROOT/sealed", Encryption::Locked),
            environment("tank/ROOT/arch", Encryption::None),
        ],
        ..Snapshot::default()
    };

    let take = |text: &str| Action::CreateSnapshot(snapshot(text));
    assert!(!take("zroot/ROOT/void@x").needs_console_on(&machine));
    assert!(take("zroot/ROOT/sealed@x").needs_console_on(&machine));
    assert!(take("tank/ROOT/arch@x").needs_console_on(&machine));
    assert!(Action::Reboot.needs_console_on(&machine));
    assert!(!Action::ResetBootTries(dataset("tank/ROOT/arch")).needs_console_on(&machine));
    let first = snapshot("zroot/ROOT/void@first");
    let new = dataset("zroot/ROOT/new");
    for change in [
        Action::Rollback(first.clone()),
        Action::Clone(first.clone(), new.clone()),
        Action::CloneAndPromote(first.clone(), new.clone()),
        Action::Duplicate(first.clone(), new),
        Action::Diff(first, None),
    ] {
        assert_eq!(
            change.changed_environment(),
            Some(&dataset("zroot/ROOT/void"))
        );
    }
    let set = Action::SetDefaultEnvironment(dataset("zroot/ROOT/void"));
    assert!(set.is_change() && !set.needs_console_on(&machine));

    // A duplicate loads the key of its target's parent too: the console is
    // taken unless that parent is known to need no key.
    let copy = |target: &str| Action::Duplicate(snapshot("zroot/ROOT/void@first"), dataset(target));
    assert!(!copy("zroot/ROOT/copy").needs_console_on(&machine));
    assert!(copy("zroot/ROOT/sealed/copy").needs_console_on(&machine));
    assert!(copy("zroot/secure/copy").needs_console_on(&machine));
}

#[test]
fn the_diff_of_two_snapshots_takes_the_console_only_for_a_locked_key() {
    use crate::model::{CommandLine, Encryption, Environment, Health, Pool, Snapshot};
    let environment = |name: &str, encryption| Environment {
        name: dataset(name),
        is_default: false,
        encryption,
        kernels: Vec::new(),
        command_line: CommandLine {
            value: Untrusted::default(),
            origin: crate::model::Origin::Default,
        },
        snapshot_count: 0,
        newest_snapshot: None,
        checks: Vec::new(),
        tries: None,
    };
    let machine = Snapshot {
        pools: vec![Pool {
            name: "tank".parse().unwrap(),
            health: Health::Online,
            writable: false,
            imported: true,
            id: None,
            host: None,
            checkpoint: Some(false),
            devices: Vec::new(),
            status: None,
            action: None,
            errors: None,
        }],
        environments: vec![
            environment("tank/ROOT/arch", Encryption::None),
            environment("tank/ROOT/sealed", Encryption::Locked),
        ],
        ..Snapshot::default()
    };
    let old = snapshot("tank/ROOT/arch@old");
    let new = snapshot("tank/ROOT/arch@new");

    assert!(!Action::Diff(old.clone(), Some(new.clone())).needs_console_on(&machine));
    assert!(Action::Diff(old.clone(), None).needs_console_on(&machine));
    let sealed = Action::Diff(
        snapshot("tank/ROOT/sealed@a"),
        Some(snapshot("tank/ROOT/sealed@b")),
    );
    assert!(sealed.needs_console_on(&machine));
    let arch = dataset("tank/ROOT/arch");
    for change in [
        Action::SetDefaultKernel(arch.clone(), Untrusted::new("6.1".to_owned())),
        Action::InheritDefaultKernel(arch.clone()),
        Action::RestoreFiles {
            snapshot: old,
            paths: Vec::new(),
        },
    ] {
        assert!(
            change.is_change() && change.needs_writable_pool(),
            "{change:?}"
        );
        assert_eq!(change.changed_environment(), Some(&arch));
        assert!(change.needs_console_on(&machine), "{change:?}");
    }
    assert!(!Action::Diff(new.clone(), Some(new)).needs_writable_pool());
}
