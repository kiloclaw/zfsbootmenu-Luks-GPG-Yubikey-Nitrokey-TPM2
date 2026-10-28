//! `zbm-zfs` against output recorded from OpenZFS 2.4.4.
//!
//! `zbm-vmtest run runtime-record-fixtures` runs the commands in
//! ZFSBootMenu's recovery shell and saves their output in `tests/fixtures`;
//! `commands.txt` lists each file's command and exit status. A fake runner
//! answers those commands with the recorded output, so the parsers and the
//! property logic see what ZFS 2.4.4 prints. The pool `tfix` has the boot
//! environments `tfix/ROOT/be`,
//! which inherits `org.zfsbootmenu:commandline` from `tfix/ROOT`, and
//! `tfix/ROOT/other`, which sets `%{parent} rw`; `tfix/ROOT/other/child`
//! inherits that value, and `tfix/secret` is encrypted.

#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests read their fixtures and fail loudly when they cannot"
)]

use std::path::{Path, PathBuf};

use zbm_config::parent::CommandLineSource;
use zbm_testkit::FakeRunner;
use zbm_types::{DatasetName, PoolName};
use zbm_zfs::mounts::{MountTable, RootFilesystem};
use zbm_zfs::{Stderr, ZfsText, parse_table};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(fixtures().join(name)).expect("the fixture exists")
}

/// A runner that answers every recorded command with its output and status.
fn recorded() -> FakeRunner {
    let runner = FakeRunner::new();
    for line in fixture("commands.txt").lines() {
        let Some((file, rest)) = line.split_once(" (exit status ") else {
            continue;
        };
        let (status, command) = rest
            .split_once("): ")
            .expect("each entry names its command");
        let status: u8 = status.parse().expect("the exit status is a number");
        runner.respond(command, status, &fixture(file));
    }
    runner
}

#[test]
fn datasets_and_properties() {
    let runner = recorded();
    let zfs = ZfsText::new(&runner);
    let name = |name: &str| name.parse::<DatasetName>().unwrap();
    assert!(zfs.exists(&name("tfix/ROOT/be")));
    assert!(zfs.is_filesystem(&name("tfix/ROOT/be")));
    let property = "org.zfsbootmenu:commandline";
    let value = |dataset: &str| zfs.value(&name(dataset), property, Stderr::Console);
    assert_eq!(value("tfix/ROOT/other").as_deref(), Some("%{parent} rw"));
    assert_eq!(value("tfix/ROOT/be").as_deref(), Some("quiet loglevel=4"));
    assert_eq!(value("tfix").as_deref(), Some("-"));
    assert_eq!(
        zfs.inherited_source("tfix/ROOT/be").as_deref(),
        Some("inherited from tfix/ROOT")
    );
    assert_eq!(zfs.inherited_source("tfix/ROOT/other").as_deref(), Some(""));

    let tfix = "tfix".parse::<PoolName>().unwrap();
    assert_eq!(zfs.imported(Stderr::Console), std::slice::from_ref(&tfix));
    assert_eq!(
        zfs.pool_value(&tfix, "bootfs", Stderr::Console).as_deref(),
        Some("tfix/ROOT/be")
    );
}

#[test]
fn parent_references_expand_through_the_recorded_properties() {
    let runner = recorded();
    let zfs = ZfsText::new(&runner);
    let mut log = Vec::new();
    assert_eq!(
        zbm_config::parent::read_kcl_prop(&zfs, "tfix/ROOT/other", &mut log).as_deref(),
        Some("quiet loglevel=4 rw")
    );
    // The child inherits "%{parent} rw" and gets the expansion at tfix/ROOT/other,
    // not a second "rw".
    assert_eq!(
        zbm_config::parent::read_kcl_prop(&zfs, "tfix/ROOT/other/child", &mut log).as_deref(),
        Some("quiet loglevel=4 rw")
    );
    assert_eq!(
        zbm_config::parent::read_kcl_prop(&zfs, "tfix/ROOT/be", &mut log).as_deref(),
        Some("quiet loglevel=4")
    );
}

#[test]
fn tables() {
    let datasets = parse_table(&fixture("zfs-list.txt"));
    let names: Vec<&str> = datasets.iter().map(|row| row[0].as_str()).collect();
    assert_eq!(
        names,
        [
            "tfix",
            "tfix/ROOT",
            "tfix/ROOT/be",
            "tfix/ROOT/other",
            "tfix/ROOT/other/child",
            "tfix/secret"
        ]
    );
    assert!(datasets.iter().all(|row| row.len() == 6));
    assert_eq!(
        datasets[2],
        ["tfix/ROOT/be", "/", "noauto", "-", "-", "quiet loglevel=4"]
    );
    assert_eq!(
        datasets[5],
        [
            "tfix/secret",
            "/secret",
            "noauto",
            "tfix/secret",
            "available",
            "-"
        ]
    );

    let snapshots = parse_table(&fixture("zfs-list-snapshots.txt"));
    assert_eq!(snapshots, [["tfix/ROOT/be@one", "0"]]);

    let pools = parse_table(&fixture("zpool-list.txt"));
    assert_eq!(pools.len(), 1);
    assert_eq!(pools[0][..2], ["tfix", "ONLINE"]);
    assert!(pools[0][2].parse::<u64>().is_ok() && pools[0][3].parse::<u64>().is_ok());
}

#[test]
fn mounts() {
    let table = MountTable::parse(&fixture("mounts.txt"));
    assert_eq!(table.mountpoint_of("tfix/ROOT/be"), Some("/mnt/be"));
    // ZFSBootMenu itself runs from its initramfs.
    assert_eq!(table.root_filesystem(), RootFilesystem::NotZfs);
    assert_eq!(table.unmount_order("/mnt/be"), ["/mnt/be"]);
    let zfs_mount = table
        .entries()
        .iter()
        .find(|entry| entry.fstype == "zfs")
        .expect("the boot environment is mounted");
    assert_eq!(zfs_mount.target, "/mnt/be");
}
