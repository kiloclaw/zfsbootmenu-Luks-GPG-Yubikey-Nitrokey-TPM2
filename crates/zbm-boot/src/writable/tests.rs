//! Tests of the read/write import.

use std::path::Path;

use zbm_platform::{Installed, KernelLog};
use zbm_testkit::fakes::FakeSystem;

use super::*;

const BCLONE: &str = "/sys/module/zfs/parameters/zfs_bclone_enabled";

/// A machine where `tank` is imported read-only with `flag` as its
/// [`READONLY`] property and no suspend partition.
fn machine(flag: &str) -> (FakeSystem, Installed) {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu").file(BCLONE, "1\n");
    let log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "on\n");
    fake.runner.respond(
        &format!("zpool get -H -o value {READONLY} tank"),
        0,
        &format!("{flag}\n"),
    );
    fake.runner.respond_program("blkid", 2, "");
    (fake, log)
}

#[test]
fn a_read_only_pool_is_exported_and_imported_read_write() {
    let (fake, _log) = machine("-");
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");

    set_rw_pool(&system, &state, "tank", ImportPolicy::Force).unwrap();

    let lines = fake.runner.lines();
    assert_eq!(
        lines[lines.len() - 2..],
        [
            "zpool export tank",
            "zpool import -N -f -o readonly=off tank"
        ]
    );
    assert_eq!(fake.files.contents(BCLONE).as_deref(), Some(&b"0"[..]));
}

#[test]
fn a_writable_pool_is_left_as_it_is() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");

    set_rw_pool(&system, &state, "tank", ImportPolicy::default()).unwrap();

    assert_eq!(fake.runner.lines(), ["zpool get -H -o value readonly tank"]);
    assert_eq!(
        set_rw_pool(&system, &state, "", ImportPolicy::default()),
        Err(BootError::NoPool)
    );
}

#[test]
fn the_readonly_property_and_a_degraded_pool_forbid_it() {
    let (fake, _log) = machine("on");
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");
    state.add_degraded("tank").unwrap();

    let error = set_rw_pool(&system, &state, "tank", ImportPolicy::default()).unwrap_err();

    assert_eq!(
        error.to_string(),
        "tank may not be imported read/write: org.zfsbootmenu:readonly is on; it is degraded"
    );
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.contains("export"))
    );

    let unreadable = FakeSystem::new();
    unreadable.runner.respond_program("zpool", 1, "");
    let system = unreadable.system();
    let state = State::new(&unreadable.files, "/zfsbootmenu");
    assert!(matches!(
        set_rw_pool(&system, &state, "tank", ImportPolicy::default()),
        Err(BootError::PoolProperty { .. })
    ));
}

#[test]
fn a_failed_import_or_export_keeps_it_read_only() {
    let (fake, _log) = machine("off");
    fake.runner.respond("zpool export tank", 1, "");
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert!(matches!(
        set_rw_pool(&system, &state, "tank", ImportPolicy::default()),
        Err(BootError::Export { .. })
    ));

    let (fake, _log) = machine("off");
    fake.runner
        .respond("zpool import -N -o readonly=off tank", 1, "");
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert!(matches!(
        set_rw_pool(&system, &state, "tank", ImportPolicy::default()),
        Err(BootError::Pool(_))
    ));
    // The export went through: the pool is imported read-only again, not
    // left exported.
    let lines = fake.runner.lines();
    assert_eq!(
        lines[lines.len() - 2..],
        [
            "zpool import -N -o readonly=off tank",
            "zpool import -N -o readonly=on tank"
        ]
    );
}
