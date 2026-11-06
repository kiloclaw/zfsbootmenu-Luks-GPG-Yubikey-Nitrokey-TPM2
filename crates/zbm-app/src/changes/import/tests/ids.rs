//! Tests of the imports of a pool named by its identifier, as when another
//! pool has its name.

use super::*;

/// `options`, naming the pool by the identifier 7.
fn by_id(options: ImportOptions) -> ImportOptions {
    ImportOptions {
        id: Some(7),
        ..options
    }
}

/// A machine where no pool is imported, and a `tank` once imported would
/// be writable.
fn not_imported() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.runner.respond("zpool list -H -o name", 0, "");
    fake.runner
        .respond(&format!("zpool get -H -o value {READONLY} tank"), 0, "-\n");
    fake.runner.respond_program("blkid", 2, "");
    fake
}

#[test]
fn a_pool_named_by_its_identifier_is_imported_by_it_every_time() {
    let fake = not_imported();
    let read_write = by_id(options(Access::ReadWrite, ImportPolicy::Strict, false));

    assert_eq!(import_tank(&fake, read_write), Ok(()));

    assert_eq!(
        moves(&fake),
        [
            "zpool import -N -o readonly=on 7",
            "zpool export tank",
            "zpool import -N -o readonly=off 7"
        ]
    );
}

#[test]
fn a_pool_named_like_an_imported_one_that_is_another_is_refused() {
    let fake = imported(false, "-");
    fake.runner
        .respond("zpool get -H -o value guid tank", 0, "1000\n");
    let read_only = by_id(options(Access::ReadOnly, ImportPolicy::Strict, false));

    let refused = import_tank(&fake, read_only);
    let planned = with_machine(&fake, false, ImportPolicy::Strict, |machine| {
        steps(machine, &tank(), read_only)
    });

    let why = vec!["another pool named tank is imported: export it first".to_owned()];
    assert_eq!(refused, Err(why.clone()));
    assert_eq!(planned, Err(why));
    assert!(moves(&fake).is_empty());
}

#[test]
fn the_imported_pool_of_the_identifier_is_imported_again_by_it() {
    let fake = imported(false, "-");
    fake.runner
        .respond("zpool get -H -o value guid tank", 0, "7\n");
    let read_write = by_id(options(Access::ReadWrite, ImportPolicy::Strict, false));

    assert_eq!(import_tank(&fake, read_write), Ok(()));

    assert_eq!(
        moves(&fake),
        ["zpool export tank", "zpool import -N -o readonly=off 7"]
    );
}

/// The configuration `zdb -e -C 7` reads of `tank`, which the host
/// `deadbeef` imported last.
const CONFIG: &str = "
MOS Configuration:
        version: 5000
        name: 'tank'
        pool_guid: 7
        hostid: 3735928559
        hostname: 'otherhost'
";

#[test]
fn a_pool_named_by_its_identifier_is_imported_with_the_host_id_its_configuration_records() {
    let fake = not_imported();
    fake.runner
        .respond("zpool import -N -o readonly=on 7", 1, "");
    fake.runner.respond("zdb -e -C 7", 0, CONFIG);
    let last_host = by_id(options(Access::ReadOnly, ImportPolicy::Hostid, false));

    // The second import is the same command line, with the host ID of the
    // pool's last host in /etc/hostid.
    let failed = import_tank(&fake, last_host).unwrap_err();
    assert_eq!(
        failed.last().map(String::as_str),
        Some("the host ID that last imported tank did not import it either")
    );
    assert_eq!(
        moves(&fake),
        [
            "zpool import -N -o readonly=on 7",
            "zpool import -N -o readonly=on 7"
        ]
    );
    let written = fake.files.contents("/etc/hostid").unwrap();
    assert_eq!(written, [0xef, 0xbe, 0xad, 0xde]);
}

#[test]
fn a_pool_whose_host_id_cannot_be_read_is_not_imported_again() {
    let fake = not_imported();
    fake.runner
        .respond("zpool import -N -o readonly=on 7", 1, "");
    let last_host = by_id(options(Access::ReadOnly, ImportPolicy::Hostid, false));

    let failed = import_tank(&fake, last_host).unwrap_err();

    assert_eq!(
        failed.last().map(String::as_str),
        Some("the host ID that last imported tank could not be read")
    );
    assert_eq!(moves(&fake), ["zpool import -N -o readonly=on 7"]);
}

#[test]
fn the_host_id_that_imports_a_pool_named_by_its_identifier_is_recorded_for_the_boot() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let fake = not_imported();
    // The first import fails, as the pool's last host is another; the one
    // with that host's ID goes through.
    let tries = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&tries);
    fake.runner.first(move |command| {
        let first = command.display() == "zpool import -N -o readonly=on 7"
            && counted.fetch_add(1, Ordering::Relaxed) == 0;
        first.then(|| Ok(Output::new(Status::exited(1), b"")))
    });
    fake.runner.respond("zdb -e -C 7", 0, CONFIG);
    let last_host = by_id(options(Access::ReadOnly, ImportPolicy::Hostid, false));

    assert_eq!(import_tank(&fake, last_host), Ok(()));

    assert_eq!(tries.load(Ordering::Relaxed), 2);
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert_eq!(state.spl_hostid().as_deref(), Some("deadbeef"));
}
