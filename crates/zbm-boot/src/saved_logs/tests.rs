use zbm_testkit::fakes::FakeSystem;

use super::*;

/// The partition table of a machine with one ESP, as `lsblk` lists it.
const ONE_ESP: &str =
    "/dev/vda \n/dev/vda1 C12A7328-F81F-11D2-BA4B-00A0C93EC93B\n/dev/vda2 6a85cf4d\n";

/// A machine with one ESP, a journal and a crash report.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.runner.respond("lsblk -rnpo NAME,PARTTYPE", 0, ONE_ESP);
    fake.runner
        .respond("dmesg", 0, "[    0.000000] Linux version\n");
    fake.files
        .file("/zfsbootmenu/journal.jsonl", "{\"message\":\"boot\"}\n")
        .file(
            "/zfsbootmenu/crash-1767225600-77.txt",
            "ZFSBootMenu crash report\n",
        )
        .file("/zfsbootmenu/bootfs", "pool/ROOT/be\n");
    fake
}

#[test]
fn stamps_are_utc_times_fat_can_name() {
    assert_eq!(stamp(0), "1970-01-01T00-00-00Z");
    assert_eq!(stamp(1_700_000_000), "2023-11-14T22-13-20Z");
    assert_eq!(stamp(951_782_400), "2000-02-29T00-00-00Z");
    assert_eq!(stamp(1_767_225_599), "2025-12-31T23-59-59Z");
}

#[test]
fn the_option_names_where_logs_go() {
    assert_eq!(Destination::from_option(None), None);
    for on in ["", "1", "on", "ESP", " yes "] {
        assert_eq!(
            Destination::from_option(Some(on)),
            Some(Destination::Esp),
            "{on}"
        );
    }
    for off in ["0", "off", "No"] {
        assert_eq!(Destination::from_option(Some(off)), None, "{off}");
    }
    assert_eq!(
        Destination::from_option(Some("PARTUUID=AB-12")),
        Some(Destination::Device("PARTUUID=AB-12".to_owned()))
    );
}

#[test]
fn the_partition_is_named_found_or_the_only_esp() {
    let fake = machine();
    fake.runner.respond(
        "blkid -c /dev/null -l -o device -t LABEL=EFI",
        0,
        "/dev/sdb1\n",
    );
    let system = fake.system();
    let named = |spec: &str| device(&system, &Destination::Device(spec.to_owned()));

    assert_eq!(named("/dev/nvme0n1p1"), Ok("/dev/nvme0n1p1".to_owned()));
    assert_eq!(named("LABEL=EFI"), Ok("/dev/sdb1".to_owned()));
    assert_eq!(
        named("LABEL=none"),
        Err(SaveError::NoDevice("LABEL=none".to_owned()))
    );
    assert_eq!(
        named("nonsense"),
        Err(SaveError::NoDevice("nonsense".to_owned()))
    );
    assert_eq!(
        device(&system, &Destination::Esp),
        Ok("/dev/vda1".to_owned())
    );

    let none = FakeSystem::new();
    assert_eq!(
        device(&none.system(), &Destination::Esp),
        Err(SaveError::NoEsp)
    );
    let two = FakeSystem::new();
    let both = format!("{ONE_ESP}/dev/vdb1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n");
    two.runner.respond("lsblk -rnpo NAME,PARTTYPE", 0, &both);
    let error = device(&two.system(), &Destination::Esp).unwrap_err();
    assert_eq!(
        error,
        SaveError::Ambiguous("/dev/vda1, /dev/vdb1".to_owned())
    );
    assert!(
        error.to_string().contains("zbm.save_logs=PARTUUID="),
        "{error}"
    );
}

#[test]
fn the_boot_loader_names_the_esp_it_loaded_from() {
    let fake = FakeSystem::new();
    let two = format!("{ONE_ESP}/dev/vdb1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n");
    fake.runner.respond("lsblk -rnpo NAME,PARTTYPE", 0, &two);
    fake.runner.respond(
        "blkid -c /dev/null -l -o device -t PARTUUID=0fc63daf-8483-4772-8e79-3d69d8477de4",
        0,
        "/dev/vdb1\n",
    );
    let mut variable = vec![7, 0, 0, 0];
    for unit in "0FC63DAF-8483-4772-8E79-3D69D8477DE4"
        .encode_utf16()
        .chain([0])
    {
        variable.extend(unit.to_le_bytes());
    }
    fake.files
        .dir("/sys/firmware/efi")
        .file(LOADER_DEVICE, variable);

    let found = device(&fake.system(), &Destination::Esp);

    assert_eq!(found, Ok("/dev/vdb1".to_owned()));
    let lines = fake.runner.lines();
    assert!(
        lines.contains(&"mountpoint -q /sys/firmware/efi/efivars".to_owned()),
        "{lines:?}"
    );
}

#[test]
fn a_save_writes_the_logs_and_keeps_the_newest_boots() {
    let fake = machine();
    let logs = Path::new(MOUNT_POINT).join(DIRECTORY);
    for day in 1..=5 {
        let old = logs.join(format!("2025-12-0{day}T00-00-00Z"));
        fake.files.file(old.join("journal.jsonl"), "old\n");
    }
    fake.files.file(logs.join("README"), "not a boot\n");
    let system = fake.system();

    let saved = save(&system, Path::new("/zfsbootmenu"), &Destination::Esp).unwrap();

    assert_eq!(saved.device, "/dev/vda1");
    assert_eq!(saved.directory, "EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z");
    assert_eq!(
        saved.files,
        ["crash-1767225600-77.txt", "journal.jsonl", "kernel.log"]
    );
    let dir = logs.join("2026-01-01T00-00-00Z");
    assert_eq!(
        fake.files.text(dir.join("journal.jsonl")).as_deref(),
        Some("{\"message\":\"boot\"}\n")
    );
    assert_eq!(
        fake.files.text(dir.join("kernel.log")).as_deref(),
        Some("[    0.000000] Linux version\n")
    );
    assert!(!fake.files.exists(&logs.join("2025-12-01T00-00-00Z")));
    assert!(
        fake.files
            .exists(&logs.join("2025-12-02T00-00-00Z/journal.jsonl"))
    );
    assert!(fake.files.exists(&logs.join("README")));
    let lines = fake.runner.lines();
    assert!(
        lines.contains(&"mount -t vfat -o rw,noatime /dev/vda1 /run/zbm-saved-logs".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"umount /run/zbm-saved-logs".to_owned()),
        "{lines:?}"
    );

    let again = save(&system, Path::new("/zfsbootmenu"), &Destination::Esp).unwrap();
    assert_eq!(
        again.directory,
        "EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z.2"
    );
    let (device, boots) = list(&system, &Destination::Esp).unwrap();
    assert_eq!(device, "/dev/vda1");
    let names: Vec<&str> = boots.iter().map(|boot| boot.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "2025-12-03T00-00-00Z",
            "2025-12-04T00-00-00Z",
            "2025-12-05T00-00-00Z",
            "2026-01-01T00-00-00Z",
            "2026-01-01T00-00-00Z.2",
        ]
    );
    let newest = boots.last().unwrap();
    assert_eq!(newest.files.len(), 3);
    assert!(newest.files.contains(&("journal.jsonl".to_owned(), 19)));
}

#[test]
fn a_partition_that_does_not_mount_is_said_so() {
    let fake = machine();
    fake.runner.respond(
        "mount -t vfat -o rw,noatime /dev/vda1 /run/zbm-saved-logs",
        32,
        "",
    );
    let system = fake.system();

    let error = save(&system, Path::new("/zfsbootmenu"), &Destination::Esp).unwrap_err();

    assert_eq!(error, SaveError::Mount("/dev/vda1".to_owned()));
    assert_eq!(error.to_string(), "unable to mount /dev/vda1");
    assert!(!fake.files.exists(Path::new(MOUNT_POINT)));
    assert!(list(&system, &Destination::Esp).is_err());
}

#[test]
fn a_file_that_cannot_be_written_is_named() {
    let fake = machine();
    let logs = Path::new(MOUNT_POINT).join(DIRECTORY);
    fake.files.fail(
        logs.join("2026-01-01T00-00-00Z/kernel.log"),
        std::io::ErrorKind::PermissionDenied,
    );

    let error = save(&fake.system(), Path::new("/zfsbootmenu"), &Destination::Esp).unwrap_err();

    assert!(
        matches!(&error, SaveError::Write(path, _) if path.ends_with("kernel.log")),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .starts_with("unable to write /run/zbm-saved-logs/"),
        "{error}"
    );
}

#[test]
fn the_boot_just_saved_stays_with_the_clock_behind() {
    let fake = machine();
    let logs = Path::new(MOUNT_POINT).join(DIRECTORY);
    // Saved by a machine whose clock was ahead: they sort after this boot.
    for day in 1..=5 {
        let later = logs.join(format!("2027-01-0{day}T00-00-00Z"));
        fake.files.file(later.join("journal.jsonl"), "later\n");
    }
    let system = fake.system();

    let saved = save(&system, Path::new("/zfsbootmenu"), &Destination::Esp).unwrap();

    assert!(
        fake.files
            .exists(&Path::new(MOUNT_POINT).join(&saved.directory))
    );
    assert!(!fake.files.exists(&logs.join("2027-01-01T00-00-00Z")));
    assert!(fake.files.exists(&logs.join("2027-01-02T00-00-00Z")));
}

#[test]
fn a_boot_not_saved_whole_is_removed_and_old_ones_pruned() {
    let fake = machine();
    let logs = Path::new(MOUNT_POINT).join(DIRECTORY);
    for day in 1..=6 {
        let old = logs.join(format!("2025-12-0{day}T00-00-00Z"));
        fake.files.file(old.join("journal.jsonl"), "old\n");
    }
    let dir = logs.join("2026-01-01T00-00-00Z");
    fake.files
        .fail(dir.join("kernel.log"), std::io::ErrorKind::StorageFull);
    let system = fake.system();

    let error = save(&system, Path::new("/zfsbootmenu"), &Destination::Esp).unwrap_err();

    assert!(error.to_string().contains("kernel.log"), "{error}");
    assert!(!fake.files.exists(&dir.join("journal.jsonl")));
    assert!(!fake.files.exists(&dir));
    assert!(!fake.files.exists(&logs.join("2025-12-01T00-00-00Z")));
    assert!(fake.files.exists(&logs.join("2025-12-02T00-00-00Z")));
}

#[test]
fn the_kernel_log_of_a_saved_boot_is_written_again_with_what_came_after() {
    let fake = machine();
    let system = fake.system();
    let saved = save(&system, Path::new("/zfsbootmenu"), &Destination::Esp).unwrap();
    let dir = Path::new(MOUNT_POINT).join(&saved.directory);
    // What the kernel logged since the save is not in the saved copy.
    fake.files.file(dir.join("kernel.log"), "stale\n");

    update_kernel_log(&system, &saved).unwrap();

    assert_eq!(
        fake.files.text(dir.join("kernel.log")).as_deref(),
        Some("[    0.000000] Linux version\n")
    );
    assert_eq!(
        fake.files.text(dir.join("journal.jsonl")).as_deref(),
        Some("{\"message\":\"boot\"}\n")
    );

    let gone = Saved {
        directory: format!("{DIRECTORY}/2020-01-01T00-00-00Z"),
        ..saved.clone()
    };
    let error = update_kernel_log(&system, &gone).unwrap_err();
    assert!(
        error.to_string().contains("the saved boot is gone"),
        "{error}"
    );

    fake.runner.respond(
        "mount -t vfat -o rw,noatime /dev/vda1 /run/zbm-saved-logs",
        32,
        "",
    );
    assert_eq!(
        update_kernel_log(&system, &saved),
        Err(SaveError::Mount("/dev/vda1".to_owned()))
    );
}
