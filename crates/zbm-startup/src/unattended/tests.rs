use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// A machine whose pool `tank` is on `/dev/mapper/luks-a`, `/dev/dm-0`, and
/// whose pool `usb` is on a disk plugged in.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .dir("/zfsbootmenu")
        .device("/dev/dm-0")
        .device("/dev/sdb1");
    fake.files.symlink("/dev/mapper/luks-a", "/dev/dm-0");
    fake.runner.respond(
        "zpool status -L -P tank",
        0,
        "  pool: tank\n config:\n\ttank  ONLINE\n\t  /dev/dm-0  ONLINE  0 0 0\n",
    );
    fake.runner.respond(
        "zpool status -L -P usb",
        0,
        "  pool: usb\n config:\n\tusb  ONLINE\n\t  /dev/sdb1  ONLINE  0 0 0\n",
    );
    fake
}

fn dataset(name: &str) -> DatasetName {
    name.parse().unwrap()
}

#[test]
fn after_an_attended_unlock_or_none_the_defaults_stay() {
    let fake = machine();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let (system, usb) = (fake.system(), dataset("usb/ROOT/evil"));
    assert_eq!(
        import_policy(&state, ImportPolicy::Hostid),
        ImportPolicy::Hostid
    );
    assert!(may_boot_by_itself(&system, &state, &usb));
    state.record_unlocked("luks-a", true).unwrap();
    assert_eq!(
        import_policy(&state, ImportPolicy::Hostid),
        ImportPolicy::Hostid
    );
    assert!(may_boot_by_itself(&system, &state, &usb));
}

#[test]
fn after_an_unattended_unlock_only_its_pools_boot_by_themselves() {
    let fake = machine();
    let state = State::new(&fake.files, "/zfsbootmenu");
    state.record_unlocked("luks-a", false).unwrap();
    let system = fake.system();

    assert_eq!(
        import_policy(&state, ImportPolicy::Hostid),
        ImportPolicy::Strict
    );
    // What an option chose stays.
    assert_eq!(
        import_policy(&state, ImportPolicy::Force),
        ImportPolicy::Force
    );
    assert!(may_boot_by_itself(
        &system,
        &state,
        &dataset("tank/ROOT/void")
    ));
    assert!(!may_boot_by_itself(
        &system,
        &state,
        &dataset("usb/ROOT/evil")
    ));
    // A pool whose devices cannot be listed boots by itself no more.
    assert!(!may_boot_by_itself(
        &system,
        &state,
        &dataset("gone/ROOT/x")
    ));
}
