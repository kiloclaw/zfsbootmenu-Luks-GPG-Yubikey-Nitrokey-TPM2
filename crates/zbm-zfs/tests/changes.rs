//! The changes `ZfsCli` makes: the command lines of the legacy functions
//! (`create_snapshot`, `rollback_snapshot`, `clone_snapshot`,
//! `duplicate_snapshot`, `set_default_env` and `zfsbootmenu-diff`), and the
//! send and receive pipeline run for real against stand-ins for `zfs` and
//! `mbuffer`.

#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests read their fixtures and fail loudly when they cannot"
)]
#![allow(clippy::unwrap_used, reason = "tests may panic")]

use std::io;
use std::path::{Path, PathBuf};

use pretty_assertions::assert_eq;
use zbm_platform::exec::{Command, Input, Output, Runner, Status, SystemRunner};
use zbm_testkit::{FakeRunner, TempDir};
use zbm_types::{DatasetName, PoolName, SnapshotName};
use zbm_zfs::{DiffChange, Format, Source, Zfs, ZfsCli, ZfsError};

fn dataset(text: &str) -> DatasetName {
    text.parse().unwrap()
}

fn snapshot(text: &str) -> SnapshotName {
    text.parse().unwrap()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn changes_run_the_legacy_command_lines() {
    let runner = FakeRunner::new();
    let zfs = ZfsCli::with_format(&runner, Format::Text);
    let pool: PoolName = "zroot".parse().unwrap();
    let first = snapshot("zroot/ROOT/void@first");
    let copy = dataset("zroot/ROOT/void_NEW");

    zfs.set_pool_property(&pool, "bootfs", "zroot/ROOT/void")
        .unwrap();
    zfs.create_snapshot(&snapshot("zroot/ROOT/void@2026-10-26-101500"))
        .unwrap();
    zfs.rollback(&first).unwrap();
    let options = pairs(&[("mountpoint", "/"), ("canmount", "noauto"), ("a:b", "c d")]);
    zfs.clone_snapshot(&first, &copy, &options).unwrap();
    zfs.promote(&copy).unwrap();
    zfs.inherit_key(&copy).unwrap();
    zfs.diff(&first, None).unwrap();
    zfs.diff(&first, Some(&snapshot("zroot/ROOT/void@second")))
        .unwrap();
    zfs.snapshot_property(&first, "creation").unwrap_err();

    let lines = runner.lines();
    assert_eq!(
        lines,
        [
            "zpool set bootfs=zroot/ROOT/void zroot",
            "zfs snapshot zroot/ROOT/void@2026-10-26-101500",
            "zfs rollback -r zroot/ROOT/void@first",
            "zfs clone -o mountpoint=/ -o canmount=noauto -o a:b=c d \
             zroot/ROOT/void@first zroot/ROOT/void_NEW",
            "zfs promote zroot/ROOT/void_NEW",
            "zfs change-key -i zroot/ROOT/void_NEW",
            "zfs diff -F -H zroot/ROOT/void@first zroot/ROOT/void",
            "zfs diff -F -H zroot/ROOT/void@first zroot/ROOT/void@second",
            "zfs get -H -p -o name,property,value,source creation zroot/ROOT/void@first",
        ]
    );
    // The value with a space is one argument.
    assert!(
        runner.calls()[3]
            .get_args()
            .iter()
            .any(|arg| arg == "a:b=c d")
    );
    assert!(
        runner
            .calls()
            .iter()
            .all(|command| *command.get_stdin() == Input::Null)
    );
}

#[test]
fn own_properties_are_the_local_and_received_ones_of_the_dataset() {
    let runner = FakeRunner::new();
    runner.respond(
        "zfs get -H -p -o name,property,value,source -s local,received all zroot/ROOT/void",
        0,
        "zroot/ROOT/void\tmountpoint\t/\tlocal\n\
         zroot/ROOT/void\torg.zfsbootmenu:commandline\tquiet\n\tro\tlocal\n\
         zroot/ROOT/void\tcompression\tzstd\treceived\n",
    );
    let zfs = ZfsCli::with_format(&runner, Format::Text);

    let own = zfs.own_properties(&dataset("zroot/ROOT/void")).unwrap();

    let shown: Vec<(&str, &str, Source)> = own
        .iter()
        .map(|(name, property)| {
            (
                name.as_str(),
                property.value.as_str(),
                property.source.clone(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("mountpoint", "/", Source::Local),
            ("org.zfsbootmenu:commandline", "quiet\n\tro", Source::Local),
            ("compression", "zstd", Source::Received),
        ]
    );
}

#[test]
fn a_failed_change_says_what_zfs_said_and_names_no_missing_dataset() {
    let runner = FakeRunner::new();
    runner.on(|command| {
        let mut output = Output::new(Status::exited(1), "");
        output.stderr = b"cannot open 'zroot/ROOT/void@x': dataset does not exist\n".to_vec();
        (command.display().contains("@x")).then_some(Ok(output))
    });
    runner.respond(
        "zfs diff -F -H zroot/ROOT/void@first zroot/ROOT/void",
        0,
        "R\tF\t/mnt/a\t/mnt/b\nM\t/\t/mnt/\n",
    );
    runner.respond(
        "zfs diff -F -H zroot/ROOT/void@odd zroot/ROOT/void",
        0,
        "?\n",
    );
    let zfs = ZfsCli::with_format(&runner, Format::Text);

    let error = zfs
        .clone_snapshot(
            &snapshot("zroot/ROOT/void@x"),
            &dataset("zroot/ROOT/new"),
            &[],
        )
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "zfs clone zroot/ROOT/void@x zroot/ROOT/new failed with status 1: \
         cannot open 'zroot/ROOT/void@x': dataset does not exist"
    );
    assert!(matches!(
        zfs.snapshot_property(&snapshot("zroot/ROOT/void@x"), "creation"),
        Err(ZfsError::Missing(name)) if name == "zroot/ROOT/void@x"
    ));
    let found = zfs.diff(&snapshot("zroot/ROOT/void@first"), None).unwrap();
    assert_eq!(found[0].change, DiffChange::Renamed);
    assert_eq!(found[0].renamed_to.as_deref(), Some("/mnt/b"));
    assert_eq!(found[1].file_type, '/');
    assert!(matches!(
        zfs.diff(&snapshot("zroot/ROOT/void@odd"), None),
        Err(ZfsError::Output { .. })
    ));
}

/// Runs commands on the system with `PATH` set to a directory of
/// stand-ins.
struct Stand(PathBuf);

impl Runner for Stand {
    fn run(&self, command: &Command) -> io::Result<Output> {
        SystemRunner.run(&command.clone().env("PATH", &self.0))
    }
}

/// A directory with `bash` and `cat`, and a `zfs` that sends `stream` (or
/// fails, when `send_fails`) and writes what it receives and its arguments
/// next to it.
fn stand_ins(send_fails: bool) -> TempDir {
    let dir = TempDir::new("zbm-zfs-send");
    for program in ["bash", "cat"] {
        let found = ["/usr/bin", "/bin"]
            .iter()
            .map(|bin| Path::new(bin).join(program))
            .find(|path| path.is_file())
            .unwrap();
        std::os::unix::fs::symlink(found, dir.join(program)).unwrap();
    }
    let out = dir.path().display();
    let send = if send_fails {
        "exit 1"
    } else {
        "printf stream"
    };
    dir.write_executable(
        "zfs",
        format!(
            "#!/bin/bash\ncase \"$1\" in\n  send) {send} ;;\n  \
             recv) shift; cat > {out}/received; echo \"$*\" > {out}/args ;;\nesac\n"
        ),
    );
    dir
}

#[test]
fn duplicating_sends_raw_into_a_receive_that_does_not_mount() {
    let dir = stand_ins(false);
    let runner = Stand(dir.path().to_owned());
    let zfs = ZfsCli::with_format(&runner, Format::Text);
    let properties = pairs(&[("canmount", "noauto"), ("keylocation", "file:///k y")]);

    zfs.send_receive(
        &snapshot("zroot/ROOT/void@first"),
        &dataset("zroot/ROOT/copy"),
        &properties,
    )
    .unwrap();

    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    assert_eq!(read("received"), "stream");
    assert_eq!(
        read("args"),
        "-u -o canmount=noauto -o keylocation=file:///k y zroot/ROOT/copy\n"
    );
    assert!(!dir.join("buffered").exists());

    dir.write_executable(
        "mbuffer",
        format!(
            "#!/bin/bash\n[ \"$1\" = -q ] || exit 9\n: > {}/buffered\nexec cat\n",
            dir.path().display()
        ),
    );
    zfs.send_receive(
        &snapshot("zroot/ROOT/void@first"),
        &dataset("zroot/ROOT/copy"),
        &[],
    )
    .unwrap();
    assert!(dir.join("buffered").exists());
    assert_eq!(read("received"), "stream");
}

#[test]
fn a_failed_send_fails_the_copy() {
    let dir = stand_ins(true);
    let runner = Stand(dir.path().to_owned());
    let zfs = ZfsCli::with_format(&runner, Format::Text);

    let error = zfs
        .send_receive(
            &snapshot("zroot/ROOT/void@first"),
            &dataset("zroot/ROOT/copy"),
            &[],
        )
        .unwrap_err();

    assert!(
        matches!(error, ZfsError::Failed { status: 1, .. }),
        "{error}"
    );
}

#[test]
fn own_properties_in_json_and_the_space_of_snapshots() {
    let runner = FakeRunner::new();
    runner.respond(
        "zfs get -j -p -o name,property,value,source -s local,received all fixture/ROOT/void",
        0,
        r#"{"datasets":{"fixture/ROOT/void":{"name":"fixture/ROOT/void","properties":{
            "mountpoint":{"value":"/","source":{"type":"LOCAL","data":"-"}},
            "compression":{"value":"zstd","source":{"type":"RECEIVED","data":"-"}}}}}}"#,
    );
    runner.respond(
        "zfs list -H -p -t snapshot -o name,used -r fixture/ROOT",
        0,
        "fixture/ROOT/void@a\t4096\nfixture/ROOT/void@b\t0\n",
    );
    runner.respond("zfs list -H -p -t snapshot -o name,used -r bad", 0, "x\n");
    runner.respond(
        "zfs get -j -p -o name,property,value,source creation fixture/ROOT/void@a",
        0,
        r#"{"datasets":{"fixture/ROOT/void@a":{"name":"fixture/ROOT/void@a","properties":{
            "creation":{"value":"1700000000","source":{"type":"NONE","data":"-"}}}}}}"#,
    );
    let zfs = ZfsCli::with_format(&runner, Format::Json);

    let mut own = zfs.own_properties(&dataset("fixture/ROOT/void")).unwrap();
    own.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(own[0].0, "compression");
    assert_eq!(own[0].1.source, Source::Received);
    assert_eq!(own[1].1.value, "/");

    let space = zfs.snapshot_space(&dataset("fixture/ROOT")).unwrap();
    assert_eq!(
        space,
        [
            (snapshot("fixture/ROOT/void@a"), 4096),
            (snapshot("fixture/ROOT/void@b"), 0)
        ]
    );
    assert!(matches!(
        zfs.snapshot_space(&dataset("bad")),
        Err(ZfsError::Output { .. })
    ));
    let creation = zfs
        .snapshot_property(&snapshot("fixture/ROOT/void@a"), "creation")
        .unwrap();
    assert_eq!(creation.value, "1700000000");
}

/// A runner that cannot start anything.
struct Broken;

impl Runner for Broken {
    fn run(&self, _command: &Command) -> io::Result<Output> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

#[test]
fn a_change_that_cannot_start_names_its_program() {
    let zfs = ZfsCli::with_format(&Broken, Format::Text);
    let error = zfs.promote(&dataset("zroot/ROOT/void")).unwrap_err();
    assert!(
        matches!(&error, ZfsError::Start { program, .. } if program == "zfs"),
        "{error}"
    );
}
