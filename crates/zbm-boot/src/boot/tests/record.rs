//! The logs and the record a boot saves before its teardown (ADR 0016,
//! point 7).

use zbm_testkit::fakes::{FakeSystem, Loads};
use zbm_types::{BootRecord, Untrusted};

use super::*;
use crate::boot::record::{HowLoaded, record, save_logs};
use crate::load::KexecLoad;
use crate::saved_logs::{Destination, MOUNT_POINT};
use zbm_platform::Files as _;
use zbm_tools::console::ConsoleInteraction;

/// The partition table of a machine with one ESP, as `lsblk` lists it.
const ONE_ESP: &str = "/dev/vda1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n";

/// Where the first save of a fake machine goes: its clock starts on
/// 2026-01-01.
const SAVED: &str = "/run/zbm-saved-logs/EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z";

/// The record of a boot of [`FS`] with the kernel 6.1.10.
fn booted() -> BootRecord {
    BootRecord {
        time: 1_767_225_600,
        environment: Untrusted::new(FS.to_owned()),
        kernel: Untrusted::new("/boot/vmlinuz-6.1.10".to_owned()),
        initramfs: Untrusted::new("/boot/initramfs-6.1.10.img".to_owned()),
        command_line: Untrusted::new(format!("root=zfs:{FS}")),
        devicetree: None,
        loaded_by: Untrusted::new("kexec_file_load".to_owned()),
        measured: false,
        command_line_edited: false,
    }
}

/// Boots [`FS`] on a machine with one ESP, with `settings`, until `kexec`
/// fails; the machine, which `prepare` prepares first.
fn boot_with(settings: &BootSettings, prepare: impl FnOnce(&FakeSystem)) -> FakeSystem {
    let _secrets = crate::testing::booting();
    let fake = FakeSystem::new();
    let _log = fake_log(&fake);
    prepare(&fake);
    fake.kernel.answer(Loads::Accepted);
    fake.runner.respond("lsblk -rnpo NAME,PARTTYPE", 0, ONE_ESP);
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "-\n");
    fake.runner.respond_program("mount", 0, "");
    fake.runner.respond_program("umount", 0, "");
    fake.runner.respond("kexec -e -i", 1, "");
    let state = State::new(&fake.files, "/zfsbootmenu");
    let selected = Kernel::new(FS, "/boot/vmlinuz-6.1.10", "/boot/initramfs-6.1.10.img");

    kexec_kernel(
        &fake.context(),
        &ConsoleInteraction::new(fake.system()),
        &state,
        settings,
        &selected,
        &Shells::default(),
    );
    fake
}

#[test]
fn a_boot_saves_its_record_with_its_logs() {
    let settings = BootSettings::default().save_logs(Some(Destination::Esp));

    let fake = boot_with(&settings, |_| {});

    let text = fake
        .files
        .text(format!("{SAVED}/boot.json"))
        .unwrap_or_else(|| panic!("{:?}", fake.files.paths()));
    let record = BootRecord::parse(text.as_bytes()).unwrap();
    assert!(
        record
            .command_line
            .get()
            .starts_with(&format!("root=zfs:{FS}")),
        "{record:?}"
    );
    assert_eq!(
        record,
        BootRecord {
            command_line: record.command_line.clone(),
            ..booted()
        }
    );
    assert!(text.ends_with("}\n"), "{text}");
    assert!(
        !fake
            .files
            .exists(Path::new(&format!("{SAVED}/boot.json.tmp")))
    );
    assert!(fake.files.exists(Path::new(&format!("{SAVED}/kernel.log"))));
}

/// Gives `fake` a TPM that takes every extension, and the boot
/// environment's kernel and initramfs to measure.
fn with_tpm(fake: &FakeSystem) {
    fake.files.device("/dev/tpmrm0");
    fake.runner
        .respond("tpm2_getcap pcrs", 0, "selected-pcrs:\n  - sha256: [ 0 ]\n");
    fake.runner.respond_program("tpm2_pcrextend", 0, "");
    let state = State::new(&fake.files, "/zfsbootmenu");
    let mnt = state.environment(FS).unwrap().join("mnt");
    fake.files
        .file(mnt.join("boot/vmlinuz-6.1.10"), b"KERNEL")
        .file(mnt.join("boot/initramfs-6.1.10.img"), b"INITRD");
}

/// The record `fake` saved.
fn saved_record(fake: &FakeSystem) -> BootRecord {
    let text = fake
        .files
        .text(format!("{SAVED}/boot.json"))
        .unwrap_or_else(|| panic!("{:?}", fake.files.paths()));
    BootRecord::parse(text.as_bytes()).unwrap()
}

#[test]
fn a_boot_that_loads_the_measured_copies_records_it_measured() {
    let settings = BootSettings::default().save_logs(Some(Destination::Esp));

    let fake = boot_with(&settings, with_tpm);

    let loaded = fake.kernel.loaded();
    assert_eq!(loaded[0].kernel, Path::new("/zfsbootmenu/measured/kernel"));
    assert!(saved_record(&fake).measured);
    let text = fake.files.text(format!("{SAVED}/boot.json")).unwrap();
    assert!(text.contains(r#""measured":true"#), "{text}");
}

#[test]
fn a_boot_that_loads_its_own_files_records_it_not_measured() {
    let settings = BootSettings::default().save_logs(Some(Destination::Esp));

    // The TPM takes the events, but the copies cannot be written: the boot
    // environment's files load, which are not what was measured.
    let fake = boot_with(&settings, |fake| {
        with_tpm(fake);
        let copy = "/zfsbootmenu/measured/initramfs";
        fake.files.fail(copy, std::io::ErrorKind::StorageFull);
    });

    let loaded = fake.kernel.loaded();
    assert!(
        loaded[0].kernel.ends_with("mnt/boot/vmlinuz-6.1.10"),
        "{loaded:?}"
    );
    let extended = fake.runner.lines();
    let extended = extended
        .iter()
        .filter(|line| line.starts_with("tpm2_pcrextend"));
    assert_eq!(extended.count(), 4);
    assert!(!saved_record(&fake).measured);
}

#[test]
fn a_boot_without_saved_logs_writes_no_record() {
    let fake = boot_with(&BootSettings::default(), |_| {});

    assert_eq!(fake.kernel.loaded().len(), 1);
    assert!(
        !fake
            .files
            .paths()
            .iter()
            .any(|path| path.starts_with(MOUNT_POINT)),
        "{:?}",
        fake.files.paths()
    );
    let lines = fake.runner.lines();
    assert!(
        !lines.iter().any(|line| line.starts_with("mount -t vfat")),
        "{lines:?}"
    );
}

#[test]
fn a_record_that_cannot_be_written_does_not_stop_the_boot() {
    let settings = BootSettings::default().save_logs(Some(Destination::Esp));

    let fake = boot_with(&settings, |fake| {
        let partial = format!("{SAVED}/boot.json.tmp");
        fake.files.fail(partial, std::io::ErrorKind::StorageFull);
    });

    // The save is warned about and removed whole; the kernel starts.
    assert!(
        fake.kmsg()
            .contains("unable to save the logs: unable to write /run/zbm-saved-logs/"),
        "{}",
        fake.kmsg()
    );
    assert!(!fake.files.exists(Path::new(SAVED)));
    assert_eq!(fake.kernel.starts(), 1);
}

#[test]
fn the_record_says_what_was_booted_and_how() {
    let selected = Kernel::new(FS, "/boot/vmlinuz-6.1.10", "/boot/initramfs-6.1.10.img");
    let root: zbm_types::KernelArg = format!("root=zfs:{FS}").parse().unwrap();
    let args: zbm_types::KernelCommandLine = "quiet".parse().unwrap();
    let mnt = "/zfsbootmenu/environments/tank/mnt";
    let dtb = Path::new(mnt).join("boot/dtbs/board.dtb");
    let load = KexecLoad {
        mnt,
        kernel: "/boot/vmlinuz-6.1.10",
        initramfs: "/boot/initramfs-6.1.10.img",
        dtb: Some(&dtb),
        root: &root,
        args: &args,
        copies: None,
    };
    let edited = BootSettings {
        command_line: Some("quiet".to_owned()),
        ..BootSettings::default()
    };
    let how = HowLoaded {
        by: "kexec",
        measured: true,
    };

    let made = record(&edited, &selected, &load, how);

    assert_eq!(
        made,
        BootRecord {
            time: 0,
            // As loaded: the root argument, the separator, the rest.
            command_line: Untrusted::new(load.command_line()),
            devicetree: Some(Untrusted::new("/boot/dtbs/board.dtb".to_owned())),
            loaded_by: Untrusted::new("kexec".to_owned()),
            measured: true,
            command_line_edited: true,
            ..booted()
        }
    );
}

#[test]
fn the_logs_are_saved_or_the_failure_logged() {
    let fake = FakeSystem::new();
    let system = fake.system();
    let state = State::new(system.files, "/zfsbootmenu");
    fake.runner.respond("lsblk -rnpo NAME,PARTTYPE", 0, ONE_ESP);

    save_logs(&Context::new(system), &state, None, Some(&booted()));
    assert!(fake.runner.lines().is_empty());
    save_logs(&Context::new(system), &state, Some(&Destination::Esp), None);
    let none = Destination::Device("LABEL=none".to_owned());
    save_logs(&Context::new(system), &state, Some(&none), None);

    let lines = fake.runner.lines();
    assert!(
        lines.contains(&"mount -t vfat -o rw,noatime /dev/vda1 /run/zbm-saved-logs".to_owned()),
        "{lines:?}"
    );
    assert!(fake.files.exists(Path::new(&format!("{SAVED}/kernel.log"))));
    // Without a record, as `zbm debug save` saves, there is none.
    assert!(!fake.files.exists(Path::new(&format!("{SAVED}/boot.json"))));
}

#[test]
fn a_failed_save_is_logged_with_its_code() {
    let fake = FakeSystem::new();
    let _log = fake_log(&fake);
    let system = fake.system();
    let state = State::new(system.files, "/zfsbootmenu");

    let saved = save_logs(
        &Context::new(system),
        &state,
        Some(&Destination::Esp),
        Some(&booted()),
    );

    assert!(saved.is_none());
    assert!(
        fake.kmsg()
            .contains("<4>ZFSBootMenu: unable to save the logs: no EFI system partition found"),
        "{}",
        fake.kmsg()
    );
}
