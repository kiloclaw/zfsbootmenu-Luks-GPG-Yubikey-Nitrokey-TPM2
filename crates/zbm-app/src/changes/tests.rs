//! Tests of package D on [`FakeZfs`], with needs that record what was
//! asked of them.

use pretty_assertions::assert_eq;
use zbm_testkit::zfs::FakeZfs;
use zbm_zfs::{DiffChange, Difference, Zfs};

use super::*;

/// Needs that record what was asked, and refuse what the test says.
#[derive(Default)]
struct FakeNeeds {
    asked: Vec<String>,
    read_only: bool,
    locked: bool,
    no_kernels: bool,
}

impl Needs for FakeNeeds {
    fn writable(&mut self, pool: &PoolName) -> Result<(), Vec<String>> {
        self.asked.push(format!("writable {pool}"));
        if self.read_only {
            return Err(vec![format!("{pool} may not be imported read/write")]);
        }
        Ok(())
    }

    fn load_key(&mut self, dataset: &DatasetName) -> Result<(), Vec<String>> {
        self.asked.push(format!("load_key {dataset}"));
        if self.locked {
            return Err(vec!["no key".to_owned()]);
        }
        Ok(())
    }

    fn has_kernels(&mut self, snapshot: &SnapshotName) -> bool {
        self.asked.push(format!("has_kernels {snapshot}"));
        !self.no_kernels
    }

    fn mount(&mut self, filesystem: &DatasetName) -> Result<String, Vec<String>> {
        self.asked.push(format!("mount {filesystem}"));
        Ok("/zfsbootmenu/environments/mnt".to_owned())
    }

    fn unmount(&mut self, mounted: &str) {
        self.asked.push(format!("unmount {mounted}"));
    }
}

fn machine() -> FakeZfs {
    FakeZfs::new()
        .pool("zroot")
        .filesystem("zroot/ROOT", &[])
        .filesystem(
            "zroot/ROOT/void",
            &[
                ("mountpoint", "/"),
                ("canmount", "noauto"),
                ("org.zfsbootmenu:commandline", "quiet"),
            ],
        )
        .snapshot("zroot/ROOT/void@first", 100)
        .snapshot("zroot/ROOT/void@second", 200)
}

fn snapshot(text: &str) -> SnapshotName {
    text.parse().unwrap()
}

fn dataset(text: &str) -> DatasetName {
    text.parse().unwrap()
}

/// Performs `action` on `zfs` with `needs`; the outcome.
fn perform(zfs: &FakeZfs, needs: &mut FakeNeeds, action: &Action) -> Vec<Event> {
    Changes { zfs, needs }.perform(action).unwrap()
}

fn outcome(events: &[Event]) -> &Outcome {
    match events.last() {
        Some(Event::Finished(outcome)) => outcome,
        other => panic!("not an outcome: {other:?}"),
    }
}

fn is_done(events: &[Event]) -> bool {
    matches!(outcome(events), Outcome::Done(_))
}

#[test]
fn a_new_default_and_a_snapshot_write_the_pool() {
    let zfs = machine();
    let mut needs = FakeNeeds::default();
    let void = dataset("zroot/ROOT/void");

    let events = perform(
        &zfs,
        &mut needs,
        &Action::SetDefaultEnvironment(void.clone()),
    );
    assert_eq!(
        outcome(&events),
        &Outcome::Done("zroot/ROOT/void is the default of zroot".to_owned())
    );
    assert_eq!(zfs.pools().unwrap()[0].bootfs, Some(void));

    let taken = snapshot("zroot/ROOT/void@2026-10-26-101500");
    let events = perform(&zfs, &mut needs, &Action::CreateSnapshot(taken.clone()));
    assert!(is_done(&events));
    assert!(zfs.snapshot_property(&taken, "creation").is_ok());
    assert_eq!(
        needs.asked,
        [
            "writable zroot",
            "writable zroot",
            "load_key zroot/ROOT/void"
        ]
    );
}

#[test]
fn what_cannot_be_prepared_fails_the_change_before_zfs_runs() {
    let zfs = machine();
    let mut needs = FakeNeeds {
        read_only: true,
        ..FakeNeeds::default()
    };
    let taken = snapshot("zroot/ROOT/void@new");

    let events = perform(&zfs, &mut needs, &Action::CreateSnapshot(taken));

    assert_eq!(
        outcome(&events),
        &Outcome::Failed {
            what: "unable to take zroot/ROOT/void@new".to_owned(),
            why: vec!["zroot may not be imported read/write".to_owned()],
            next: Vec::new(),
        }
    );
    assert!(!zfs.calls().iter().any(|call| call.starts_with("create")));

    let mut locked = FakeNeeds {
        locked: true,
        ..FakeNeeds::default()
    };
    let events = perform(
        &zfs,
        &mut locked,
        &Action::Rollback(snapshot("zroot/ROOT/void@first")),
    );
    assert!(!is_done(&events));
    assert!(!zfs.calls().iter().any(|call| call.starts_with("rollback")));
}

#[test]
fn a_rollback_needs_kernels_and_destroys_the_newer_snapshots() {
    let zfs = machine();
    let first = snapshot("zroot/ROOT/void@first");
    let mut bare = FakeNeeds {
        no_kernels: true,
        ..FakeNeeds::default()
    };

    let events = perform(&zfs, &mut bare, &Action::Rollback(first.clone()));
    let Outcome::Failed { what, .. } = outcome(&events) else {
        panic!("rolled back to a snapshot without kernels");
    };
    assert_eq!(
        what,
        "zroot/ROOT/void@first has no kernels, will not roll back"
    );
    assert_eq!(bare.asked, ["has_kernels zroot/ROOT/void@first"]);

    let mut needs = FakeNeeds::default();
    let events = perform(&zfs, &mut needs, &Action::Rollback(first));
    assert!(is_done(&events));
    let left = zfs.snapshots(&dataset("zroot/ROOT/void")).unwrap();
    assert_eq!(left.len(), 1);
}

#[test]
fn clones_take_the_environments_own_properties() {
    let zfs = machine()
        .filesystem("zroot/ROOT/void", &[("keylocation", "prompt")])
        .snapshot("zroot/home@x", 1)
        .filesystem("zroot/home", &[]);
    let mut needs = FakeNeeds::default();
    let second = snapshot("zroot/ROOT/void@second");
    let copy = dataset("zroot/ROOT/void_NEW");

    let events = perform(
        &zfs,
        &mut needs,
        &Action::Clone(second.clone(), copy.clone()),
    );

    assert!(is_done(&events));
    let clone_call = zfs
        .calls()
        .into_iter()
        .find(|call| call.starts_with("clone_snapshot"))
        .unwrap();
    assert_eq!(
        clone_call,
        "clone_snapshot zroot/ROOT/void@second zroot/ROOT/void_NEW \
         mountpoint=/,canmount=noauto,org.zfsbootmenu:commandline=quiet"
    );
    assert_eq!(
        zfs.origin_of("zroot/ROOT/void_NEW").as_deref(),
        Some("zroot/ROOT/void@second")
    );

    let promoted = dataset("zroot/ROOT/void_P");
    let events = perform(&zfs, &mut needs, &Action::CloneAndPromote(second, promoted));
    assert!(is_done(&events));
    assert_eq!(zfs.origin_of("zroot/ROOT/void_P"), None);
    assert!(
        zfs.calls()
            .contains(&"promote zroot/ROOT/void_P".to_owned())
    );

    let elsewhere = dataset("tank/void");
    let events = perform(
        &zfs,
        &mut needs,
        &Action::Clone(snapshot("zroot/home@x"), elsewhere),
    );
    let Outcome::Failed { why, .. } = outcome(&events) else {
        panic!("cloned into another pool");
    };
    assert_eq!(why, &["tank/void is not in zroot"]);

    let events = perform(
        &zfs,
        &mut needs,
        &Action::Clone(snapshot("zroot/ROOT/void@first"), copy),
    );
    assert!(!is_done(&events), "the clone exists");
}

#[test]
fn a_duplicate_of_an_encrypted_child_takes_its_parents_key() {
    let zfs = FakeZfs::new()
        .pool("zroot")
        .filesystem(
            "zroot/ROOT",
            &[
                ("encryptionroot", "zroot/ROOT"),
                ("keylocation", "file:///etc/zfs/k"),
            ],
        )
        .filesystem("zroot/ROOT/void", &[("encryptionroot", "zroot/ROOT")])
        .snapshot("zroot/ROOT/void@first", 100);
    let mut needs = FakeNeeds::default();
    let copy = dataset("zroot/ROOT/copy");

    let events = perform(
        &zfs,
        &mut needs,
        &Action::Duplicate(snapshot("zroot/ROOT/void@first"), copy.clone()),
    );

    assert!(is_done(&events), "{events:?}");
    assert!(
        zfs.calls().contains(
            &"send_receive zroot/ROOT/void@first zroot/ROOT/copy \
          mountpoint=/,canmount=noauto,keylocation=file:///etc/zfs/k"
                .to_owned()
        )
    );
    assert_eq!(
        zfs.property(&copy, "encryptionroot").unwrap().value,
        "zroot/ROOT"
    );
    assert_eq!(
        needs.asked,
        [
            "writable zroot",
            "load_key zroot/ROOT/void",
            "load_key zroot/ROOT",
            "load_key zroot/ROOT/copy",
        ]
    );
}

#[test]
fn a_duplicate_needs_room_and_an_unencrypted_one_keeps_no_key() {
    let full = machine()
        .filesystem("zroot/ROOT", &[("available", "1024")])
        .filesystem("zroot/ROOT/void", &[("referenced", "1048576")]);
    let mut needs = FakeNeeds::default();
    let first = snapshot("zroot/ROOT/void@first");

    let events = perform(
        &full,
        &mut needs,
        &Action::Duplicate(first.clone(), dataset("zroot/ROOT/c")),
    );
    assert_eq!(
        outcome(&events),
        &Outcome::Failed {
            what: "insufficient space for duplication".to_owned(),
            why: vec!["zroot/ROOT has 1.0 KiB free but needs 1.0 MiB".to_owned()],
            next: Vec::new(),
        }
    );

    let zfs = machine();
    let events = perform(
        &zfs,
        &mut needs,
        &Action::Duplicate(first, dataset("zroot/ROOT/c")),
    );
    assert!(is_done(&events));
    assert!(
        !zfs.calls()
            .iter()
            .any(|call| call.starts_with("inherit_key"))
    );
    assert_eq!(bytes(5), "5 B");
    assert_eq!(bytes(1536 * 1024 * 1024), "1.5 GiB");
}

#[test]
fn a_diff_compares_the_older_snapshot_with_the_newer_below_the_mount() {
    let found = [
        Difference {
            change: DiffChange::Modified,
            file_type: '/',
            path: "/zfsbootmenu/environments/mnt".to_owned(),
            renamed_to: None,
        },
        Difference {
            change: DiffChange::Renamed,
            file_type: 'F',
            path: "/zfsbootmenu/environments/mnt/etc/a".to_owned(),
            renamed_to: Some("/zfsbootmenu/environments/mnt/etc/b".to_owned()),
        },
    ];
    let zfs = machine().differences(
        "zroot/ROOT/void@first",
        Some("zroot/ROOT/void@second"),
        &found,
    );
    let mut needs = FakeNeeds::default();
    let first = snapshot("zroot/ROOT/void@first");
    let second = snapshot("zroot/ROOT/void@second");

    let events = perform(
        &zfs,
        &mut needs,
        &Action::Diff(second.clone(), Some(first.clone())),
    );

    let [
        Event::DiffReady {
            from,
            to,
            changes,
            packages,
        },
    ] = events.as_slice()
    else {
        panic!("no differences: {events:?}");
    };
    assert_eq!((from, to.as_ref()), (&first, Some(&second)));
    assert_eq!(packages, &None);
    assert_eq!(changes[0].path, "/");
    assert_eq!(changes[1].kind, ChangeKind::Renamed);
    assert_eq!(changes[1].path, "/etc/a");
    assert_eq!(changes[1].renamed_to.as_deref(), Some("/etc/b"));
    assert_eq!(
        needs.asked.last().map(String::as_str),
        Some("unmount /zfsbootmenu/environments/mnt")
    );

    let failing = machine().failing("diff", "not mounted");
    let events = perform(&failing, &mut needs, &Action::Diff(first, None));
    assert!(!is_done(&events));
    assert_eq!(change(found[1].clone(), "/other").path, found[1].path);
}

#[test]
fn other_actions_are_not_changes() {
    let zfs = machine();
    let mut needs = FakeNeeds::default();
    assert!(
        Changes {
            zfs: &zfs,
            needs: &mut needs
        }
        .perform(&Action::Reboot)
        .is_none()
    );
}

#[test]
fn a_change_takes_the_console_on_a_read_only_pool_or_a_locked_environment() {
    use crate::model::{CommandLine, Encryption, Environment, Health, Origin, Pool, Snapshot};
    let pool = |name: &str, writable| Pool {
        name: name.parse().unwrap(),
        health: Health::Online,
        writable,
        imported: true,
        foreign_hostid: None,
        checkpoint: false,
    };
    let environment = |name: &str, encryption| Environment {
        name: dataset(name),
        is_default: false,
        encryption,
        kernels: Vec::new(),
        command_line: CommandLine {
            value: String::new(),
            origin: Origin::Default,
        },
        snapshots: Vec::new(),
        checks: Vec::new(),
        tries: None,
    };
    let machine = Snapshot {
        pools: vec![pool("zroot", true), pool("tank", false)],
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
fn what_zfs_refuses_fails_the_change_with_its_message() {
    let first = snapshot("zroot/ROOT/void@first");
    let new = dataset("zroot/ROOT/new");
    let cases = [
        (
            "set_pool_property",
            Action::SetDefaultEnvironment(dataset("zroot/ROOT/void")),
        ),
        (
            "create_snapshot",
            Action::CreateSnapshot(snapshot("zroot/ROOT/void@x")),
        ),
        ("rollback", Action::Rollback(first.clone())),
        ("clone_snapshot", Action::Clone(first.clone(), new.clone())),
        (
            "promote",
            Action::CloneAndPromote(first.clone(), new.clone()),
        ),
        (
            "send_receive",
            Action::Duplicate(first.clone(), new.clone()),
        ),
        ("diff", Action::Diff(first.clone(), None)),
    ];
    for (operation, action) in cases {
        let zfs = machine().failing(operation, "refused");
        let events = perform(&zfs, &mut FakeNeeds::default(), &action);
        let Outcome::Failed { why, .. } = outcome(&events) else {
            panic!("{operation} did not fail");
        };
        assert!(why[0].ends_with("refused"), "{operation}: {why:?}");
    }

    // A clone of a filesystem that went away, and a copy whose parent's
    // key does not load.
    let gone = FakeZfs::new()
        .pool("zroot")
        .snapshot("zroot/ROOT/void@first", 1);
    let events = perform(
        &gone,
        &mut FakeNeeds::default(),
        &Action::Clone(first.clone(), new.clone()),
    );
    assert!(!is_done(&events));
    let mut locked = FakeNeeds {
        locked: true,
        ..FakeNeeds::default()
    };
    let events = perform(
        &machine(),
        &mut locked,
        &Action::Duplicate(first.clone(), new),
    );
    assert!(!is_done(&events));
    let events = perform(&machine(), &mut locked, &Action::Diff(first, None));
    assert!(!is_done(&events));
}

#[test]
fn a_copy_that_cannot_take_its_parents_key_says_so() {
    let zfs = FakeZfs::new()
        .pool("zroot")
        .filesystem("zroot/ROOT", &[("encryptionroot", "zroot/ROOT")])
        .filesystem("zroot/ROOT/void", &[("encryptionroot", "zroot/ROOT")])
        .snapshot("zroot/ROOT/void@first", 100)
        .failing("inherit_key", "refused");

    let events = perform(
        &zfs,
        &mut FakeNeeds::default(),
        &Action::Duplicate(
            snapshot("zroot/ROOT/void@first"),
            dataset("zroot/ROOT/copy"),
        ),
    );

    let Outcome::Failed { what, .. } = outcome(&events) else {
        panic!("the copy kept its own key");
    };
    assert_eq!(what, "unable to give zroot/ROOT/copy its parent's key");
}

#[test]
fn removed_and_created_paths_are_named_as_such() {
    let path = |change, path: &str| Difference {
        change,
        file_type: 'F',
        path: path.to_owned(),
        renamed_to: None,
    };
    assert_eq!(
        change(path(DiffChange::Removed, "/m/a"), "/m").kind,
        ChangeKind::Removed
    );
    assert_eq!(
        change(path(DiffChange::Created, "/m/b"), "/m").kind,
        ChangeKind::Created
    );
}
