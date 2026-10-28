//! `ZfsCli` against output recorded from OpenZFS (`cargo xtask record-zfs`):
//! the JSON and the text of the same pool give the same answers.
//!
//! The recordings are in `tests/fixtures/<major.minor>/`, one file per
//! command of `xtask/src/record_zfs.rs`. A fake runner answers each command
//! that `ZfsCli` runs with the recording of the same listing, in the format
//! the test chose.

#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests read their fixtures and fail loudly when they cannot"
)]
#![allow(clippy::unwrap_used, reason = "a missing fixture fails the test")]

use std::path::{Path, PathBuf};

use pretty_assertions::assert_eq;
use zbm_platform::{Output, Status};
use zbm_testkit::FakeRunner;
use zbm_types::{DatasetName, PoolName};
use zbm_zfs::{Format, Health, Property, Source, Zfs, ZfsCli, ZfsError};

/// The releases recorded.
const RELEASES: &[&str] = &["2.4"];

/// The properties of `filesystems.*`, without `name`.
const DATASET_PROPERTIES: &[&str] = &[
    "mountpoint",
    "canmount",
    "encryptionroot",
    "keystatus",
    "keylocation",
    "keyformat",
    "org.zfsbootmenu:kernel",
    "org.zfsbootmenu:commandline",
];

fn fixtures(release: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(release)
}

fn fixture(release: &str, name: &str) -> String {
    std::fs::read_to_string(fixtures(release).join(name))
        .unwrap_or_else(|error| panic!("fixture {release}/{name}: {error}"))
}

/// A runner that answers the commands of `ZfsCli` in `format` with the
/// recordings of `release`.
fn machine(release: &str, format: Format) -> FakeRunner {
    let (flag, extension) = match format {
        Format::Json => ("-j -p", "json"),
        Format::Text => ("-H -p", "txt"),
    };
    let runner = FakeRunner::new();
    let answer = |command: String, file: &str| {
        runner.respond(
            &command,
            0,
            &fixture(release, &format!("{file}.{extension}")),
        );
    };
    answer(
        format!("zpool list {flag} -o name,health,readonly,bootfs,guid"),
        "pools",
    );
    answer(
        format!(
            "zfs list {flag} -t filesystem -o name,{} -r fixture",
            DATASET_PROPERTIES.join(",")
        ),
        "filesystems",
    );
    answer(
        format!("zfs list {flag} -t snapshot -o name,creation -r fixture"),
        "snapshots",
    );
    for property in ["mountpoint", "canmount", "org.zfsbootmenu:commandline"] {
        let command =
            format!("zfs get {flag} -o name,property,value,source {property} fixture/ROOT/void");
        let all = fixture(release, &format!("properties.{extension}"));
        runner.respond(&command, 0, &all);
    }
    for property in ["readonly", "bootfs", "guid"] {
        let command = format!("zpool get {flag} -o name,property,value,source {property} fixture");
        let all = fixture(release, &format!("pool-properties.{extension}"));
        runner.respond(&command, 0, &all);
    }
    runner.respond("zfs version -j", 0, &fixture(release, "version.json"));
    runner.respond("zfs version", 0, &fixture(release, "version.txt"));
    runner.respond(
        &format!("zfs list {flag} -t filesystem -o name,mountpoint -r fixture/none"),
        1,
        "",
    );
    runner
}

fn dataset(name: &str) -> DatasetName {
    name.parse().unwrap()
}

#[test]
fn both_formats_describe_the_same_pool() {
    for release in RELEASES {
        let json_runner = machine(release, Format::Json);
        let text_runner = machine(release, Format::Text);
        let json = ZfsCli::with_format(&json_runner, Format::Json);
        let text = ZfsCli::with_format(&text_runner, Format::Text);

        let pools = json.pools().unwrap();
        assert_eq!(pools, text.pools().unwrap(), "{release}");
        assert_eq!(pools.len(), 1);
        assert_eq!(pools[0].name.as_str(), "fixture");
        assert_eq!(pools[0].health, Health::Online);
        assert!(!pools[0].read_only);
        assert_eq!(pools[0].bootfs, Some(dataset("fixture/ROOT/void")));
        assert!(pools[0].guid > 0);

        let root = dataset("fixture");
        let from_json = json.filesystems(Some(&root), DATASET_PROPERTIES).unwrap();
        let from_text = text.filesystems(Some(&root), DATASET_PROPERTIES).unwrap();
        let names: Vec<_> = from_json.iter().map(|d| d.name.to_string()).collect();
        assert_eq!(
            names,
            [
                "fixture",
                "fixture/ROOT",
                "fixture/ROOT/arch",
                "fixture/ROOT/secret",
                "fixture/ROOT/void",
                "fixture/home"
            ]
        );
        // Text has no sources: compare the values.
        for (json_dataset, text_dataset) in from_json.iter().zip(&from_text) {
            assert_eq!(json_dataset.name, text_dataset.name);
            for property in DATASET_PROPERTIES {
                assert_eq!(
                    json_dataset.value(property),
                    text_dataset.value(property),
                    "{release} {} {property}",
                    json_dataset.name
                );
            }
        }
        let secret = &from_json[3];
        assert_eq!(secret.value("keystatus"), Some("unavailable"));
        assert_eq!(secret.value("encryptionroot"), Some("fixture/ROOT/secret"));
        assert_eq!(from_json[2].value("org.zfsbootmenu:kernel"), Some("6.1"));
        assert_eq!(
            secret.properties["org.zfsbootmenu:commandline"].source,
            Source::Inherited("fixture/ROOT".to_owned())
        );

        let snapshots = json.snapshots(&root).unwrap();
        assert_eq!(snapshots, text.snapshots(&root).unwrap(), "{release}");
        assert_eq!(snapshots.len(), 2);
        assert!(snapshots[0].creation > 1_700_000_000);
    }
}

#[test]
fn properties_have_their_sources_in_both_formats() {
    for release in RELEASES {
        for format in [Format::Json, Format::Text] {
            let runner = machine(release, format);
            let zfs = ZfsCli::with_format(&runner, format);
            let void = dataset("fixture/ROOT/void");

            assert_eq!(
                zfs.property(&void, "mountpoint").unwrap(),
                Property::new("/", Source::Local)
            );
            assert_eq!(
                zfs.property(&void, "org.zfsbootmenu:commandline").unwrap(),
                Property::new(
                    "quiet loglevel=4",
                    Source::Inherited("fixture/ROOT".to_owned())
                )
            );
            let pool: PoolName = "fixture".parse().unwrap();
            assert_eq!(zfs.pool_property(&pool, "readonly").unwrap().value, "off");
            assert_eq!(
                zfs.pool_property(&pool, "bootfs").unwrap(),
                Property::new("fixture/ROOT/void", Source::Local)
            );
            let version = zfs.version().unwrap();
            assert_eq!(version.release(), Some((2, 4)), "{format:?}");
            assert!(version.has_json());
        }
    }
}

#[test]
fn read_only_pools_and_missing_datasets() {
    for release in RELEASES {
        for (format, extension) in [(Format::Json, "json"), (Format::Text, "txt")] {
            let runner = FakeRunner::new();
            let flag = if format == Format::Json {
                "-j -p"
            } else {
                "-H -p"
            };
            runner.respond(
                &format!("zpool list {flag} -o name,health,readonly,bootfs,guid"),
                0,
                &fixture(release, &format!("readonly-pools.{extension}")),
            );
            let missing = fixture(release, "missing.txt.err");
            runner.on(move |command| {
                command.display().ends_with("fixture/none").then(|| {
                    let mut output = Output::new(Status::exited(1), "");
                    output.stderr = missing.clone().into_bytes();
                    Ok(output)
                })
            });
            let zfs = ZfsCli::with_format(&runner, format);

            assert!(zfs.pools().unwrap()[0].read_only, "{release} {format:?}");
            assert!(matches!(
                zfs.filesystems(Some(&dataset("fixture/none")), &["mountpoint"]),
                Err(ZfsError::Missing(name)) if name == "fixture/none"
            ));
        }
    }
}

#[test]
fn the_format_is_chosen_once_from_zfs_version() {
    let runner = FakeRunner::new();
    runner.respond("zfs version -j", 0, &fixture("2.4", "version.json"));
    let zfs = ZfsCli::new(&runner);
    assert_eq!(zfs.format(), Format::Json);
    assert_eq!(zfs.format(), Format::Json);
    assert_eq!(runner.lines(), ["zfs version -j"]);

    let old = FakeRunner::new();
    old.respond("zfs version -j", 2, "");
    assert_eq!(ZfsCli::new(&old).format(), Format::Text);
}

#[test]
fn failures_say_what_failed() {
    let runner = FakeRunner::new();
    runner.on(|command| {
        let line = command.display();
        if line.starts_with("zpool list") {
            Some(Err(std::io::Error::other("no such program")))
        } else if line.starts_with("zfs set") {
            let mut output = Output::new(Status::exited(1), "");
            output.stderr = b"cannot set property: permission denied\n".to_vec();
            Some(Ok(output))
        } else if line.starts_with("zfs list") {
            Some(Ok(Output::new(Status::exited(0), "not json")))
        } else if line.starts_with("zfs get") {
            Some(Ok(Output::new(Status::exited(0), "{\"datasets\":{}}")))
        } else {
            None
        }
    });
    let zfs = ZfsCli::with_format(&runner, Format::Json);
    let void = dataset("fixture/ROOT/void");

    assert_eq!(
        zfs.pools().unwrap_err().to_string(),
        "unable to run zpool: no such program"
    );
    assert_eq!(
        zfs.set(&void, "canmount", "on").unwrap_err().to_string(),
        "zfs set canmount=on fixture/ROOT/void failed with status 1: \
         cannot set property: permission denied"
    );
    assert!(matches!(
        zfs.snapshots(&void),
        Err(ZfsError::Output { command, .. }) if command.starts_with("zfs list -j -p")
    ));
    assert!(matches!(
        zfs.property(&void, "canmount"),
        Err(ZfsError::Output { .. })
    ));
    zfs.inherit(&void, "canmount").unwrap();
    assert!(zfs.filesystems(None, &[]).is_err());
    let lines = runner.lines();
    assert!(lines.contains(&"zfs inherit canmount fixture/ROOT/void".to_owned()));
    assert!(lines.contains(&"zfs list -j -p -t filesystem -o name".to_owned()));
}

#[test]
fn old_versions_answer_in_text() {
    let runner = FakeRunner::new();
    runner.respond("zfs version", 0, "zfs-2.2.7-1\nzfs-kmod-2.2.7-1\n");
    runner.respond("zfs version -j", 2, "");
    runner.respond("zfs version -j", 2, "");
    let zfs = ZfsCli::new(&runner);

    let version = zfs.version().unwrap();
    assert_eq!(version.release(), Some((2, 2)));
    assert_eq!(zfs.format(), Format::Text);
    let bad = FakeRunner::new();
    bad.respond("zfs version", 0, "unknown\n");
    assert!(ZfsCli::with_format(&bad, Format::Text).version().is_err());
    let bad_json = FakeRunner::new();
    bad_json.respond("zfs version -j", 0, "{}");
    assert!(
        ZfsCli::with_format(&bad_json, Format::Json)
            .version()
            .is_err()
    );
}

#[test]
fn pool_names_devices_and_export_need_no_format() {
    let runner = FakeRunner::new();
    runner.respond("zpool list -H -o name", 0, "fixture\nrpool\n");
    runner.respond(
        "zpool status -L -P fixture",
        0,
        "  pool: fixture\n state: ONLINE\nconfig:\n\n\tNAME           STATE\n\
         \tfixture        ONLINE\n\t  mirror-0     ONLINE\n\
         \t    /dev/dm-3  ONLINE       0     0     0\n\
         \t    /dev/vda2  ONLINE       0     0     0\n\nerrors: No known data errors\n",
    );
    runner.on(|command| {
        let mut output = Output::new(Status::exited(1), "");
        output.stderr = b"cannot open 'gone': no such pool\n".to_vec();
        command.display().ends_with(" gone").then_some(Ok(output))
    });
    let zfs = ZfsCli::new(&runner);
    let fixture: PoolName = "fixture".parse().unwrap();
    let gone: PoolName = "gone".parse().unwrap();

    let names = zfs.pool_names().unwrap();
    assert_eq!(names, [fixture.clone(), "rpool".parse().unwrap()]);
    assert_eq!(zfs.devices(&fixture).unwrap(), ["/dev/dm-3", "/dev/vda2"]);
    assert!(matches!(zfs.devices(&gone), Err(ZfsError::Missing(name)) if name == "gone"));
    zfs.export(&fixture).unwrap();
    assert!(zfs.export(&gone).is_err());
    assert_eq!(
        runner.lines(),
        [
            "zpool list -H -o name",
            "zpool status -L -P fixture",
            "zpool status -L -P gone",
            "zpool export fixture",
            "zpool export gone"
        ]
    );
    let bad = FakeRunner::new();
    bad.respond("zpool list -H -o name", 0, "tank/ROOT\n");
    assert!(matches!(
        ZfsCli::new(&bad).pool_names(),
        Err(ZfsError::Output { .. })
    ));
    // A name the port refuses is left out; the other pools stay listed.
    let one_bad = FakeRunner::new();
    one_bad.respond("zpool list -H -o name", 0, "rpool\ntank/ROOT\nfixture\n");
    let names = ZfsCli::new(&one_bad).pool_names().unwrap();
    assert_eq!(names, ["rpool".parse().unwrap(), fixture]);
}
