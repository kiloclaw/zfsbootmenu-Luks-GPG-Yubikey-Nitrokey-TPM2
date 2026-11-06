//! Tests of the snapshots of an environment loaded on demand, and read
//! again with every snapshot of the machine for the column open.

use std::sync::atomic::Ordering;

use pretty_assertions::assert_eq;
use zbm_platform::Context;
use zbm_platform::exec::{Output, Status};
use zbm_testkit::fakes::FakeSystem;

use super::super::*;
use crate::auth::{self, Authority};
use crate::{Application as _, Runtime, Seen, Through};

/// The listing of the snapshots of `environment`.
fn listing(environment: &str) -> String {
    format!("zfs list -H -p -t snapshot -o name,creation,used -d 1 -s creation {environment}")
}

/// A machine with two boot environments, `tank/ROOT/be` with two
/// snapshots and `tank/ROOT/other` with one, in text.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner.respond("zfs version -j", 1, "");
    fake.runner.respond(
        "zfs list -H -p -t filesystem -o name,mountpoint,canmount,keystatus",
        0,
        "tank/ROOT/be\t/\tnoauto\t-\ntank/ROOT/other\t/\tnoauto\t-\n",
    );
    fake.runner.respond(
        &listing("tank/ROOT/be"),
        0,
        "tank/ROOT/be@old\t100\t4096\ntank/ROOT/be@new\t200\t0\n",
    );
    fake.runner.respond(
        &listing("tank/ROOT/other"),
        0,
        "tank/ROOT/other@one\t50\t1\n",
    );
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 1, "");
    fake
}

/// Makes the snapshots of `environment` read `text` from now on.
fn now_lists(fake: &FakeSystem, environment: &str, text: &'static str) {
    let line = listing(environment);
    fake.runner.first(move |command| {
        (command.display() == line).then(|| Ok(Output::new(Status::exited(0), text.as_bytes())))
    });
}

/// The names of the snapshots `list` holds, in its order.
fn names(list: &SnapshotList) -> Vec<String> {
    list.snapshots
        .iter()
        .map(|snapshot| snapshot.name.name().to_owned())
        .collect()
}

/// The list of `events`, which must be one [`Event::SnapshotsLoaded`].
fn loaded(events: Vec<Event>) -> SnapshotList {
    match <[Event; 1]>::try_from(events) {
        Ok([Event::SnapshotsLoaded(list)]) => list,
        other => panic!("{other:?}"),
    }
}

fn load(environment: &str) -> Action {
    Action::LoadSnapshots(environment.parse().unwrap())
}

#[test]
fn every_snapshot_after_a_load_reads_the_open_column_again() {
    let fake = machine();
    let runtime = Runtime::default();
    let seen = Seen::new();
    let app = App::new(Context::new(fake.system()), &runtime).with_seen(&seen);
    let before = app.snapshot().unwrap();
    assert!(before.loaded.is_empty());
    let be = &before.environments[0];
    assert_eq!(be.snapshot_count, 2);
    assert_eq!(
        be.newest_snapshot.as_ref().map(|s| s.name.name()),
        Some("new")
    );

    let list = loaded(app.apply(&load("tank/ROOT/be"), Through::Quietly(fake.system())));
    assert_eq!(names(&list), ["new", "old"]);
    assert_eq!(app.snapshot().unwrap().loaded, [list]);

    // A change made meanwhile shows in the next snapshot: the list is read
    // again, never kept from an older read.
    now_lists(&fake, "tank/ROOT/be", "tank/ROOT/be@old\t100\t4096\n");
    let after = app.snapshot().unwrap();
    assert_eq!(after.loaded.len(), 1);
    assert_eq!(names(&after.loaded[0]), ["old"]);
    assert_eq!(after.environments[0].snapshot_count, 1);

    // Another column replaces it, on the console as quietly.
    let other = loaded(app.apply(
        &load("tank/ROOT/other"),
        Through::Console(fake.system(), &crate::application::tests::NoShell),
    ));
    assert_eq!(app.snapshot().unwrap().loaded, [other]);
}

#[test]
fn a_snapshot_lists_the_open_column_once_and_counts_from_that_list() {
    let fake = machine();
    let runtime = Runtime::default();
    let seen = Seen::new();
    let app = App::new(Context::new(fake.system()), &runtime).with_seen(&seen);
    loaded(app.apply(&load("tank/ROOT/be"), Through::Quietly(fake.system())));
    let before = fake.runner.lines().len();

    let snapshot = app.snapshot().unwrap();

    let listed = |environment: &str| {
        fake.runner.lines()[before..]
            .iter()
            .filter(|line| **line == listing(environment))
            .count()
    };
    assert_eq!((listed("tank/ROOT/be"), listed("tank/ROOT/other")), (1, 1));
    let be = &snapshot.environments[0];
    assert_eq!(be.snapshot_count, 2);
    assert_eq!(
        be.newest_snapshot.as_ref().map(|s| s.name.name()),
        Some("new")
    );
    assert_eq!(snapshot.loaded[0].snapshots.len(), 2);
}

#[test]
fn a_column_read_again_in_vain_is_counted_all_the_same() {
    let fake = machine();
    let runtime = Runtime::default();
    let seen = Seen::new();
    let app = App::new(Context::new(fake.system()), &runtime).with_seen(&seen);
    loaded(app.apply(&load("tank/ROOT/be"), Through::Quietly(fake.system())));
    // Its list cannot be read again, but its count still can.
    let listed = std::sync::atomic::AtomicUsize::new(0);
    let line = listing("tank/ROOT/be");
    fake.runner.first(move |command| {
        let first = listed.fetch_add(usize::from(command.display() == line), Ordering::SeqCst);
        (command.display() == line && first == 0)
            .then(|| Ok(Output::new(Status::exited(1), b"cannot open".as_slice())))
    });

    let snapshot = app.snapshot().unwrap();

    assert!(snapshot.loaded.is_empty());
    assert_eq!(snapshot.environments[0].snapshot_count, 2);
}

#[test]
fn a_load_that_fails_says_why_and_closes_the_column() {
    let fake = machine();
    let runtime = Runtime::default();
    let seen = Seen::new();
    let app = App::new(Context::new(fake.system()), &runtime).with_seen(&seen);
    loaded(app.apply(&load("tank/ROOT/be"), Through::Quietly(fake.system())));

    let events = app.apply(&load("tank/ROOT/gone"), Through::Quietly(fake.system()));

    let [
        Event::SnapshotsUnread(unread),
        Event::Finished(Outcome::Failed { what, why, .. }),
    ] = events.as_slice()
    else {
        panic!("{events:?}");
    };
    assert_eq!(unread.to_string(), "tank/ROOT/gone");
    assert_eq!(what, "unable to read the snapshots of tank/ROOT/gone");
    assert_eq!(why.len(), 1);
    assert!(app.snapshot().unwrap().loaded.is_empty());
}

#[test]
fn a_column_whose_environment_went_is_read_no_more() {
    let fake = machine();
    let runtime = Runtime::default();
    let seen = Seen::new();
    let app = App::new(Context::new(fake.system()), &runtime).with_seen(&seen);
    loaded(app.apply(&load("tank/ROOT/be"), Through::Quietly(fake.system())));
    let before = fake.runner.lines().len();

    fake.runner.first(|command| {
        (command.display() == "zfs list -H -p -t filesystem -o name,mountpoint,canmount,keystatus")
            .then(|| {
                Ok(Output::new(
                    Status::exited(0),
                    b"tank/ROOT/other\t/\tnoauto\t-\n".as_slice(),
                ))
            })
    });
    let snapshot = app.snapshot().unwrap();

    assert!(snapshot.loaded.is_empty());
    let listed = fake.runner.lines()[before..]
        .iter()
        .filter(|line| **line == listing("tank/ROOT/be"))
        .count();
    assert_eq!(listed, 0);
    // Without anything to keep the column in, a snapshot has none.
    let alone = App::new(Context::new(fake.system()), &runtime);
    loaded(alone.apply(&load("tank/ROOT/be"), Through::Quietly(fake.system())));
    assert!(alone.snapshot().unwrap().loaded.is_empty());
}

#[test]
fn the_locked_menu_lets_the_snapshots_be_read() {
    let fake = machine();
    fake.files.file("/zfsbootmenu/bootfs", "tank/ROOT/be\n");
    let runtime = Runtime::default();
    let authority = Authority::new(auth::Lock::On("hash".to_owned()));
    let app = App::new(Context::new(fake.system()), &runtime).with_authority(&authority);

    assert!(app.snapshot().unwrap().menu_locked);
    let list = loaded(app.apply(&load("tank/ROOT/be"), Through::Quietly(fake.system())));
    assert_eq!(list.snapshots.len(), 2);
}
