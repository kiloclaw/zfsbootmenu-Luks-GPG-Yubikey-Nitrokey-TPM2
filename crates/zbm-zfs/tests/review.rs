//! `ZfsCli` reading what a review of a change shows before it is made: the
//! snapshots and bookmarks of a dataset with their holds and clones, the
//! space a destroy would free, and the checkpoint of a pool, from the
//! hand-written fixtures of `tests/fixtures/written/`, whose README says
//! what each shows; and the command lines it runs for them.

#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests read their fixtures and fail loudly when they cannot"
)]
#![allow(clippy::unwrap_used, reason = "tests may panic")]

use std::path::Path;

use pretty_assertions::assert_eq;
use zbm_platform::exec::Input;
use zbm_platform::{Output, Status};
use zbm_testkit::FakeRunner;
use zbm_types::{DatasetName, PoolName, SnapshotName};
use zbm_zfs::{SnapshotSet, Zfs, ZfsCli, ZfsError};

/// A hand-written fixture of `tests/fixtures/written/`.
fn written(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/written")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// A runner on which `command` fails with `stderr`.
fn failing(command: &'static str, stderr: &'static str) -> FakeRunner {
    let runner = FakeRunner::new();
    runner.on(move |run| {
        (run.display() == command).then(|| {
            let mut output = Output::new(Status::exited(1), "");
            output.stderr = stderr.as_bytes().to_vec();
            Ok(output)
        })
    });
    runner
}

fn snapshot(text: &str) -> SnapshotName {
    text.parse().unwrap()
}

fn be() -> DatasetName {
    "tank/ROOT/be".parse().unwrap()
}

const DETAILS: &str = "zfs list -H -p -t snapshot,bookmark -o name,createtxg,used,userrefs,clones \
                       -d 1 -s createtxg tank/ROOT/be";

/// A detail as a test compares it: name, createtxg, used and userrefs.
type Row = (String, u64, Option<u64>, u64);

#[test]
fn the_details_of_snapshots_are_in_the_order_zfs_took_them() {
    let runner = FakeRunner::new();
    runner.respond(DETAILS, 0, &written("snapshot-details.txt"));

    let details = ZfsCli::new(&runner).snapshot_details(&be()).unwrap();

    let shown: Vec<Row> = details
        .iter()
        .map(|detail| {
            (
                detail.name.to_string(),
                detail.createtxg,
                detail.used,
                detail.userrefs,
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("tank/ROOT/be@install".to_owned(), 52, Some(1_966_080), 0),
            ("tank/ROOT/be#install".to_owned(), 52, None, 0),
            ("tank/ROOT/be@2026-10-22".to_owned(), 1873, Some(409_600), 1),
            ("tank/ROOT/be@before-upgrade".to_owned(), 2216, Some(0), 0),
            ("tank/ROOT/be@after-upgrade".to_owned(), 2216, Some(8192), 2),
        ]
    );
    let clones: Vec<&str> = details[4]
        .clones
        .iter()
        .map(|clone| clone.get().as_str())
        .collect();
    assert_eq!(clones, ["tank/ROOT/be-copy", "tank/ROOT/be-old"]);
    assert!(details[0].clones.is_empty());
    assert_eq!(runner.lines(), [DETAILS]);
    let missing = failing(
        DETAILS,
        "cannot open 'tank/ROOT/be': dataset does not exist\n",
    );
    assert!(matches!(
        ZfsCli::new(&missing).snapshot_details(&be()),
        Err(ZfsError::Missing(name)) if name == "tank/ROOT/be"
    ));
}

#[test]
fn holds_are_asked_of_the_snapshots_named_and_of_none_without_running_zfs() {
    let runner = FakeRunner::new();
    let command = "zfs holds -H -p tank/ROOT/be@2026-10-22 tank/ROOT/be@after-upgrade";
    runner.respond(command, 0, &written("holds.txt"));
    let zfs = ZfsCli::new(&runner);

    let holds = zfs
        .holds(&[
            snapshot("tank/ROOT/be@2026-10-22"),
            snapshot("tank/ROOT/be@after-upgrade"),
        ])
        .unwrap();
    let none = zfs.holds(&[]).unwrap();

    let shown: Vec<(&str, &str)> = holds
        .iter()
        .map(|hold| (hold.snapshot.name(), hold.tag.get().as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("2026-10-22", "keep"),
            ("after-upgrade", "backup"),
            ("after-upgrade", "send .1792700001"),
        ]
    );
    assert!(none.is_empty());
    assert_eq!(runner.lines(), [command]);
}

#[test]
fn a_missing_snapshot_fails_the_holds_without_naming_another() {
    let command = "zfs holds -H -p tank/ROOT/be@gone tank/ROOT/be@after-upgrade";
    let missing = failing(
        command,
        "cannot open 'tank/ROOT/be@gone': dataset does not exist\n",
    );

    let read = ZfsCli::new(&missing).holds(&[
        snapshot("tank/ROOT/be@gone"),
        snapshot("tank/ROOT/be@after-upgrade"),
    ]);

    assert!(matches!(
        read,
        Err(ZfsError::Failed { message, .. }) if message.contains("tank/ROOT/be@gone")
    ));
}

#[test]
fn the_space_a_destroy_frees_is_counted_with_n_and_nothing_is_destroyed() {
    let runner = FakeRunner::new();
    let range = "zfs destroy -n -v -p tank/ROOT/be@2026-10-22%after-upgrade";
    runner.respond(range, 0, &written("destroy-dry.txt"));
    runner.respond(
        "zfs destroy -n -v -p tank/ROOT/be@a,b",
        0,
        "destroy\ttank/ROOT/be@a\ndestroy\ttank/ROOT/be@b\nreclaim\t0\n",
    );
    let zfs = ZfsCli::new(&runner);

    let freed = zfs
        .destroy_space(&SnapshotSet::Range {
            first: snapshot("tank/ROOT/be@2026-10-22"),
            last: snapshot("tank/ROOT/be@after-upgrade"),
        })
        .unwrap();
    let nothing = zfs
        .destroy_space(&SnapshotSet::Each(vec![
            snapshot("tank/ROOT/be@a"),
            snapshot("tank/ROOT/be@b"),
        ]))
        .unwrap();

    assert_eq!((freed, nothing), (425_984, 0));
    // The exact arguments: `-n` before any name, each name one argument.
    let args: Vec<Vec<String>> = runner
        .calls()
        .iter()
        .map(|call| {
            call.get_args()
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        })
        .collect();
    assert_eq!(
        args,
        [
            [
                "destroy",
                "-n",
                "-v",
                "-p",
                "tank/ROOT/be@2026-10-22%after-upgrade"
            ],
            ["destroy", "-n", "-v", "-p", "tank/ROOT/be@a,b"],
        ]
    );
    assert!(
        runner
            .calls()
            .iter()
            .all(|command| *command.get_stdin() == Input::Null)
    );
}

#[test]
fn a_destroy_count_zfs_refuses_or_garbles_is_an_error() {
    let command = "zfs destroy -n -v -p tank/ROOT/be@a";
    let refused = failing(
        command,
        "cannot destroy snapshot tank/ROOT/be@a: snapshot has dependent clones\n",
    );
    let garbled = FakeRunner::new();
    garbled.respond(command, 0, "would reclaim 4K\n");
    let one = SnapshotSet::Each(vec![snapshot("tank/ROOT/be@a")]);

    assert!(matches!(
        ZfsCli::new(&refused).destroy_space(&one),
        Err(ZfsError::Failed { message, .. }) if message.contains("dependent clones")
    ));
    assert!(matches!(
        ZfsCli::new(&garbled).destroy_space(&one),
        Err(ZfsError::Output { command: shown, .. }) if shown == command
    ));
}

#[test]
fn snapshots_are_destroyed_in_one_command() {
    let runner = FakeRunner::new();
    let zfs = ZfsCli::new(&runner);

    zfs.destroy_snapshots(&[snapshot("tank/ROOT/be@a"), snapshot("tank/ROOT/be@b c")])
        .unwrap();

    assert_eq!(runner.lines(), ["zfs destroy tank/ROOT/be@a,b c"]);
    assert_eq!(runner.calls()[0].get_args().len(), 2);
}

#[test]
fn the_checkpoint_is_read_from_the_status_of_its_pool() {
    let runner = FakeRunner::new();
    runner.respond("zpool status -p tank", 0, &written("status-checkpoint.txt"));
    runner.respond(
        "zpool status -p data",
        0,
        "  pool: data\n state: ONLINE\nconfig:\n\n\tdata  ONLINE  0 0 0\n",
    );
    let zfs = ZfsCli::new(&runner);
    let pool = |name: &str| name.parse::<PoolName>().unwrap();

    let checkpoint = zfs.checkpoint(&pool("tank")).unwrap().unwrap();
    let none = zfs.checkpoint(&pool("data")).unwrap();

    assert_eq!(
        checkpoint.created.unwrap().get(),
        "Fri Oct 23 08:32:23 2026"
    );
    assert_eq!(checkpoint.space, Some(1_572_864));
    assert!(!checkpoint.discarding);
    assert_eq!(none, None);
    assert!(
        runner.calls()[0]
            .get_env()
            .contains(&("LC_ALL".into(), Some("C".into())))
    );
    let missing = failing("zpool status -p tank", "cannot open 'tank': no such pool\n");
    assert!(matches!(
        ZfsCli::new(&missing).checkpoint(&pool("tank")),
        Err(ZfsError::Failed { .. })
    ));
}
