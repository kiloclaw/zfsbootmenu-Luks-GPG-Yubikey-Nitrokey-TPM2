//! `ZfsCli` reading what pools are made of: `zpool status`, the listing
//! of `zpool import` and `zdb -e -C -u`, from the recordings of `cargo xtask
//! record-zfs` (`tests/fixtures/<major.minor>/`) and the hand-written
//! fixtures of `tests/fixtures/written/`, whose README says what each
//! shows.

#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests read their fixtures and fail loudly when they cannot"
)]
#![allow(clippy::unwrap_used, reason = "a missing fixture fails the test")]

use std::path::Path;

use pretty_assertions::assert_eq;
use zbm_platform::{Output, Status};
use zbm_testkit::FakeRunner;
use zbm_zfs::{Format, Health, VdevClass, VdevErrors, VdevKind, Zfs, ZfsCli, ZfsError};

/// The releases recorded.
const RELEASES: &[&str] = &["2.4"];

fn fixture(release: &str, name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(release)
        .join(name);
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("fixture {release}/{name}: {error}"))
}

/// A hand-written fixture of `tests/fixtures/written/`.
fn written(name: &str) -> String {
    fixture("written", name)
}

/// A runner on which `command` fails with `status` and `stderr`.
fn failing(command: &'static str, status: u8, stderr: String) -> FakeRunner {
    let runner = FakeRunner::new();
    runner.on(move |run| {
        (run.display() == command).then(|| {
            let mut output = Output::new(Status::exited(status), "");
            output.stderr = stderr.clone().into_bytes();
            Ok(output)
        })
    });
    runner
}

#[test]
fn the_status_of_the_recorded_pool_is_the_same_tree_in_both_formats() {
    for release in RELEASES {
        let runner = FakeRunner::new();
        runner.respond("zpool status -j -p -P", 0, &fixture(release, "status.json"));
        runner.respond("zpool status -p -P", 0, &fixture(release, "status.txt"));

        let json = ZfsCli::with_format(&runner, Format::Json).status().unwrap();
        let text = ZfsCli::with_format(&runner, Format::Text).status().unwrap();

        assert_eq!(json, text, "{release}");
        let pool = &json[0];
        assert_eq!(pool.name.as_str(), "fixture");
        assert_eq!((pool.state.clone(), pool.errors), (Health::Online, Some(0)));
        assert_eq!((pool.status.as_ref(), pool.action.as_ref()), (None, None));
        let [disk] = pool.vdevs.as_slice() else {
            panic!("{release}: one disk, not {:?}", pool.vdevs);
        };
        assert_eq!(disk.name.get(), "/dev/disk/by-id/virtio-d0-part1");
        assert_eq!((disk.kind, disk.class), (VdevKind::Device, VdevClass::Data));
        assert_eq!(disk.errors, Some(VdevErrors::default()));
        assert!(disk.children.is_empty() && disk.note.is_none());
        for call in runner.calls() {
            assert!(
                call.get_env()
                    .contains(&("LC_ALL".into(), Some("C".into()))),
                "{}",
                call.display()
            );
        }
    }
}

#[test]
fn the_recorded_listing_of_an_exported_pool_is_read() {
    for release in RELEASES {
        let runner = FakeRunner::new();
        runner.respond("zpool import", 0, &fixture(release, "exported.txt"));

        let listed = ZfsCli::new(&runner).importable().unwrap();

        let [pool] = listed.as_slice() else {
            panic!("{release}: one pool, not {listed:?}");
        };
        assert_eq!(pool.name.as_str(), "fixture");
        assert_eq!(pool.id, 4_438_165_986_505_091_419);
        assert_eq!(pool.state, Health::Online);
        assert!(!pool.foreign && pool.status.is_none());
        assert_eq!(
            pool.action.as_ref().unwrap().get(),
            "The pool can be imported using its name or numeric identifier."
        );
        let [disk] = pool.vdevs.as_slice() else {
            panic!("{release}: one disk, not {:?}", pool.vdevs);
        };
        assert_eq!(disk.name.get(), "virtio-d0");
        assert_eq!(
            (disk.state.clone(), disk.errors),
            (Some(Health::Online), None)
        );
        // The listing needs no format.
        assert_eq!(runner.lines(), ["zpool import"]);
    }
}

#[test]
fn a_degraded_mirror_and_every_class_are_read_from_the_text() {
    let runner = FakeRunner::new();
    runner.respond("zpool status -p -P", 0, &written("status-degraded.txt"));

    let pools = ZfsCli::with_format(&runner, Format::Text).status().unwrap();

    let names: Vec<&str> = pools.iter().map(|pool| pool.name.as_str()).collect();
    assert_eq!(names, ["tank", "rpool", "scratch"]);
    let tank = &pools[0];
    assert_eq!(
        (tank.state.clone(), tank.errors),
        (Health::Degraded, Some(2))
    );
    assert_eq!(
        tank.status.as_ref().unwrap().get(),
        "One or more devices could not be opened.  Sufficient replicas exist for \
         the pool to continue functioning in a degraded state."
    );
    let mirror = &tank.vdevs[0];
    assert_eq!(
        (mirror.kind, mirror.state.clone()),
        (VdevKind::Mirror, Some(Health::Degraded))
    );
    let [online, missing] = mirror.children.as_slice() else {
        panic!("two disks, not {:?}", mirror.children);
    };
    assert_eq!(online.errors.unwrap().cksum, 3);
    assert_eq!(missing.name.get(), "9876543210987654321");
    assert_eq!(missing.state, Some(Health::Unavailable));
    assert_eq!(
        missing.note.as_ref().unwrap().get(),
        "was /dev/disk/by-id/virtio-d1-part1"
    );
    let classes: Vec<(VdevClass, &str)> = tank.vdevs[1..]
        .iter()
        .map(|vdev| (vdev.class, vdev.name.get().as_str()))
        .collect();
    assert_eq!(
        classes,
        [
            (VdevClass::Special, "/dev/disk/by-id/virtio-d4-part1"),
            (VdevClass::Log, "/dev/disk/by-id/virtio-d2-part1"),
            (VdevClass::Cache, "/dev/disk/by-id/virtio-d3-part1"),
            (VdevClass::Spare, "/dev/disk/by-id/virtio-d5-part1"),
        ]
    );
    let spare = &tank.vdevs[4];
    assert_eq!(
        (spare.state.clone(), spare.errors),
        (Some(Health::Other("AVAIL".into())), None)
    );
    assert_eq!(pools[1].errors, Some(0));
    assert_eq!(pools[1].vdevs.len(), 1);
    let scratch = &pools[2];
    let faulted = &scratch.vdevs[0].children[1];
    assert_eq!(faulted.state, Some(Health::Faulted));
    assert_eq!(faulted.errors.unwrap().read, 12);
    assert_eq!(faulted.note.as_ref().unwrap().get(), "too many errors");
    let spare = &scratch.vdevs[1];
    assert_eq!(
        (spare.class, spare.state.clone()),
        (VdevClass::Spare, Some(Health::Unavailable))
    );
    assert_eq!(spare.note.as_ref().unwrap().get(), "cannot open");
}

#[test]
fn unhealthy_pools_with_every_class_are_the_same_in_both_formats() {
    let runner = FakeRunner::new();
    runner.respond("zpool status -j -p -P", 0, &written("status-degraded.json"));
    runner.respond("zpool status -p -P", 0, &written("status-degraded.txt"));

    let json = ZfsCli::with_format(&runner, Format::Json).status().unwrap();
    let text = ZfsCli::with_format(&runner, Format::Text).status().unwrap();

    assert_eq!(json, text);
    assert_eq!(json.len(), 3);
    let classes: Vec<VdevClass> = json[0].vdevs.iter().map(|vdev| vdev.class).collect();
    assert_eq!(
        classes,
        [
            VdevClass::Data,
            VdevClass::Special,
            VdevClass::Log,
            VdevClass::Cache,
            VdevClass::Spare,
        ]
    );
}

#[test]
fn foreign_pools_and_pools_of_one_name_are_listed() {
    let runner = FakeRunner::new();
    runner.respond("zpool import", 0, &written("exported-foreign.txt"));

    let listed = ZfsCli::new(&runner).importable().unwrap();

    let summary: Vec<(&str, u64, bool)> = listed
        .iter()
        .map(|pool| (pool.name.as_str(), pool.id, pool.foreign))
        .collect();
    assert_eq!(
        summary,
        [
            ("tank", 15_094_445_427_306_487_906, true),
            ("data", 2_306_016_271_538_451_322, false),
            ("data", 9_165_323_530_912_380_101, false),
        ]
    );
    assert_eq!(
        listed[0].action.as_ref().unwrap().get(),
        "The pool can be imported using its name or numeric identifier and the '-f' flag."
    );
    let data = &listed[1];
    assert_eq!(data.state, Health::Degraded);
    let missing = &data.vdevs[0].children[1];
    assert_eq!(missing.state, Some(Health::Unavailable));
    assert_eq!(missing.note.as_ref().unwrap().get(), "cannot open");
    assert_eq!(data.vdevs[1].class, VdevClass::Log);
    assert_eq!(data.vdevs[1].name.get(), "vde");

    let old = FakeRunner::new();
    old.respond("zpool import", 0, &written("exported-2.1.txt"));
    let listed_by_2_1 = ZfsCli::new(&old).importable().unwrap();
    assert_eq!(listed_by_2_1.as_slice(), &listed[..1]);
}

#[test]
fn no_pool_to_import_is_an_empty_list_and_other_failures_are_errors() {
    let none = failing("zpool import", 0, written("no-pools.err"));
    assert!(ZfsCli::new(&none).importable().unwrap().is_empty());
    for call in none.calls() {
        assert!(
            call.get_env()
                .contains(&("LC_ALL".into(), Some("C".into())))
        );
    }

    // A scan that fails says nothing.
    let broken = failing("zpool import", 1, String::new());
    assert!(matches!(
        ZfsCli::new(&broken).importable(),
        Err(ZfsError::Failed { status: 1, .. })
    ));
    let garbled = FakeRunner::new();
    garbled.respond(
        "zpool import",
        0,
        "  pool: a/b\n    id: 1\n state: ONLINE\n",
    );
    assert!(matches!(
        ZfsCli::new(&garbled).importable(),
        Err(ZfsError::Output { command, .. }) if command == "zpool import"
    ));
    let empty = FakeRunner::new();
    empty.respond("zpool status -p -P", 0, "");
    assert!(
        ZfsCli::with_format(&empty, Format::Text)
            .status()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_host_of_a_pool_not_imported_comes_from_its_configuration() {
    let runner = FakeRunner::new();
    let command = "zdb -e -C -u 15094445427306487906";
    runner.respond(command, 0, &written("zdb-config.txt"));

    let host = ZfsCli::new(&runner)
        .pool_host(15_094_445_427_306_487_906)
        .unwrap();

    assert_eq!(host.hostid.unwrap().as_str(), "deadbeef");
    assert_eq!(host.hostname.unwrap().get(), "otherhost");
    assert_eq!(host.written, Some(1_792_667_543));
    let [call] = runner.calls().try_into().unwrap();
    assert!(
        call.get_env()
            .contains(&("LC_ALL".into(), Some("C".into())))
    );
    let missing = failing(
        "zdb -e -C -u 7",
        1,
        "zdb: can't open '7': No such file or directory\n".to_owned(),
    );
    assert!(matches!(
        ZfsCli::new(&missing).pool_host(7),
        Err(ZfsError::Failed { .. })
    ));
    let garbled = FakeRunner::new();
    garbled.respond("zdb -e -C -u 7", 0, "hostid: 1\n");
    assert!(matches!(
        ZfsCli::new(&garbled).pool_host(7),
        Err(ZfsError::Output { command, .. }) if command == "zdb -e -C -u 7"
    ));
}
