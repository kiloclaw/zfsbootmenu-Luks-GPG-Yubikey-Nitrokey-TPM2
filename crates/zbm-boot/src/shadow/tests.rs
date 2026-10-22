//! Tests of shadow mode: each decision on machines where the two runtimes
//! agree and where they do not, and that shadow mode only reads.

use std::path::Path;
use std::time::Duration;

use zbm_core::exec::{Output, Status};
use zbm_platform::ReadOnlyRunner;
use zbm_state::State;
use zbm_system::System;
use zbm_testkit::fakes::FakeSystem;
use zbm_testkit::machine::FakeMachine;

use super::*;

const BASE: &str = "/zfsbootmenu";

fn state(fake: &FakeSystem) -> State<'_> {
    fake.files.dir(BASE);
    State::new(&fake.files, BASE)
}

/// Answers `zpool get -H -o value readonly POOL` with `on` for each pool.
fn read_only_pools(fake: &FakeSystem) {
    fake.runner.on(|command| {
        let line = command.display();
        line.starts_with("zpool get -H -o value readonly ")
            .then(|| Ok(Output::new(Status::exited(0), "on\n")))
    });
}

#[test]
fn the_host_id_is_the_matched_one_or_the_one_given() {
    let fake = FakeSystem::new();
    let state = state(&fake);
    fake.files.file("/etc/hostid", [0xef, 0xbe, 0xad, 0xde]);

    assert_eq!(host_id(&fake.files, &state), None);

    fake.files
        .file("/etc/zfsbootmenu.conf", "export spl_hostid='deadbeef'\n");
    assert_eq!(
        host_id(&fake.files, &state),
        Some(Comparison {
            decision: "host ID",
            legacy: "deadbeef".to_owned(),
            rust: "deadbeef".to_owned()
        })
    );

    state.set_spl_hostid("0bab10c").unwrap();
    let matched = host_id(&fake.files, &state).unwrap();
    assert_eq!(matched.rust, "00bab10c");
    assert!(matched.is_difference());
}

#[test]
fn pools_are_read_only_and_only_the_required_one_is_imported() {
    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool list -H -o name", 0, "tank\ndata\n");
    read_only_pools(&fake);

    let both = pools(&fake.runner, &fake.files).unwrap();
    assert_eq!(both.legacy, "tank=on data=on");
    assert!(!both.is_difference());

    fake.files.file(
        "/etc/zfsbootmenu.conf",
        "export zbm_prefer_pool='tank'\nexport zbm_require_pool='only'\n",
    );
    let only = pools(&fake.runner, &fake.files).unwrap();
    assert_eq!(only.rust, "tank=on");
    assert!(only.is_difference());

    let none = FakeSystem::new();
    assert_eq!(pools(&none.runner, &none.files), None);
}

#[test]
fn luks_mappings_are_named_by_the_crypttab_or_the_uuid() {
    let machine = FakeMachine::new()
        .with_luks_volume("/dev/sda2", "pw")
        .with_luks_volume("/dev/sdb2", "pw")
        .with_file("/etc/zfsbootmenu/crypttab", "root /dev/sda2 none\n");
    let uuid = machine.volume("/dev/sdb2").uuid;
    let state = state(&machine);
    machine.files.file(
        "/zfsbootmenu/luks-mappings",
        format!("root\t/dev/sda2\t0\nluks-{uuid}\t/dev/sdb2\t1\n"),
    );

    let same = luks_mappings(&machine.system(), &state).unwrap();

    assert_eq!(same.legacy, format!("/dev/sda2=root /dev/sdb2=luks-{uuid}"));
    assert!(!same.is_difference(), "{same:?}");

    machine
        .files
        .file("/zfsbootmenu/luks-mappings", "other\t/dev/sda2\t0\n");
    let renamed = luks_mappings(&machine.system(), &state).unwrap();
    assert_eq!(renamed.rust, "/dev/sda2=root");
    assert!(renamed.is_difference());
}

#[test]
fn the_default_boot_environment_comes_from_bootfs() {
    let fake = FakeSystem::new();
    let state = state(&fake);
    fake.runner
        .respond("zpool list -H -o bootfs --", 0, "-\ntank/ROOT/void\n");

    assert_eq!(boot_environment(&fake.runner, &fake.files, &state), None);
    state.set_bootfs("tank/ROOT/void").unwrap();
    let comparison = boot_environment(&fake.runner, &fake.files, &state).unwrap();

    assert_eq!(comparison.rust, "tank/ROOT/void");
    assert!(!comparison.is_difference());
}

#[test]
fn the_kernel_is_the_newest_unless_the_property_says_otherwise() {
    let fake = FakeSystem::new();
    for name in [
        "vmlinuz-6.1.0",
        "initramfs-6.1.0.img",
        "vmlinuz-6.6.30",
        "initramfs-6.6.30.img",
    ] {
        fake.files.file(Path::new("/mnt/be/boot").join(name), "");
    }
    fake.runner.respond(
        "zfs get -H -o value org.zfsbootmenu:kernel tank/ROOT/void",
        0,
        "-\n",
    );
    let selected = |kernel: &str, initramfs: &str| Selected {
        be: "tank/ROOT/void".to_owned(),
        kernel: kernel.to_owned(),
        initramfs: initramfs.to_owned(),
        mountpoint: "/mnt/be".to_owned(),
    };

    let newest = kernel(
        &fake.system(),
        &selected("/boot/vmlinuz-6.6.30", "/boot/initramfs-6.6.30.img"),
    )
    .unwrap();
    let older = kernel(
        &fake.system(),
        &selected("/boot/vmlinuz-6.1.0", "/boot/initramfs-6.1.0.img"),
    )
    .unwrap();

    assert!(!newest.is_difference(), "{newest:?}");
    assert!(older.is_difference());
    assert_eq!(kernel(&fake.system(), &Selected::default()), None);
}

#[test]
fn a_report_counts_differences_and_shadow_mode_only_reads() {
    let fake = FakeSystem::new();
    let state = state(&fake);
    fake.files.file("/etc/hostid", [0x0c, 0xb1, 0xba, 0x00]);
    state.set_spl_hostid("deadbeef").unwrap();
    fake.runner.respond("zpool list -H -o name", 0, "tank\n");
    read_only_pools(&fake);
    let runner = ReadOnlyRunner::new(&fake.runner, &fake.clock, Duration::from_secs(5));
    let system = System {
        runner: &runner,
        ..fake.system()
    };

    let report = compare(&system, &state, &Selected::default());

    assert_eq!(report.comparisons.len(), 2);
    assert_eq!(report.differences(), 1);
    assert_eq!(
        report.text(),
        concat!(
            "ZBM-SHADOW: host ID: legacy 00bab10c, rust deadbeef\n",
            "ZBM-SHADOW: 1 differences in 2 decisions (host ID, pools)\n",
        )
    );
    assert!(fake.runner.calls().iter().all(zbm_platform::is_read_only));
}

#[test]
fn the_summary_goes_to_an_efi_variable_once_and_is_read_back_once() {
    let fake = FakeSystem::new();
    let state = state(&fake);
    let efivars = Path::new("/sys/firmware/efi/efivars");
    fake.files.dir(efivars);

    assert!(efivar::record(&fake.files, &state, efivars, "ZBM-SHADOW: 1 differences\n").unwrap());
    assert!(!efivar::record(&fake.files, &state, efivars, "again").unwrap());
    let raw = fake.files.contents(efivar::path(efivars)).unwrap();
    assert_eq!(&raw[..4], &[7, 0, 0, 0]);

    assert_eq!(
        efivar::take(&fake.files, efivars).unwrap().as_deref(),
        Some("ZBM-SHADOW: 1 differences\n")
    );
    assert_eq!(efivar::take(&fake.files, efivars).unwrap(), None);
    assert!(
        efivar::record(
            &FakeSystem::new().files,
            &state,
            Path::new("/nonexistent"),
            "x"
        )
        .is_err()
    );
}

#[test]
fn the_selection_comes_from_the_hook_environment() {
    let fake = FakeSystem::new();
    fake.environment
        .set("ZBM_SELECTED_BE", "tank/ROOT/void")
        .set("ZBM_SELECTED_KERNEL", "/boot/vmlinuz")
        .set("ZBM_SELECTED_INITRAMFS", "/boot/initramfs.img")
        .set("ZBM_SELECTED_MOUNTPOINT", "/mnt");

    let selected = Selected::from_environment(&fake.environment);

    assert_eq!(selected.be, "tank/ROOT/void");
    assert_eq!(selected.mountpoint, "/mnt");
    assert_eq!(selected.initramfs, "/boot/initramfs.img");
}
