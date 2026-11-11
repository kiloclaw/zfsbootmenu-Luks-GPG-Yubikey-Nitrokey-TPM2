use std::io;

use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// The partition table of a machine with one ESP, as `lsblk` lists it.
const ONE_ESP: &str =
    "/dev/vda \n/dev/vda1 C12A7328-F81F-11D2-BA4B-00A0C93EC93B\n/dev/vda2 6a85cf4d\n";

/// The kernel's mounts before any partition is.
const MOUNTED: &str = "proc /proc proc rw 0 0\ntank/ROOT/be /zfsbootmenu/mnt zfs ro 0 0\n";

/// A machine whose `/dev/vdb` is a block device, with nothing mounted at
/// `/mnt/vdb`.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .block_device("/dev/vdb")
        .block_device("/dev/vda1")
        .file(MOUNTS, MOUNTED);
    fake
}

/// The device `path`.
fn device(path: &str) -> DevicePath {
    path.parse().unwrap()
}

#[test]
fn the_esp_is_found_by_its_partuuid_or_its_type() {
    let fake = machine();
    fake.runner.respond("lsblk -rnpo NAME,PARTTYPE", 0, ONE_ESP);

    assert_eq!(
        mount_esp(&fake.system(), None).unwrap(),
        Path::new("/mnt/vda1")
    );
    assert_eq!(
        fake.runner.lines().last().unwrap(),
        "mount /dev/vda1 /mnt/vda1"
    );

    // The boot loader names the one it loaded from among two.
    let fake = machine();
    let two = format!("{ONE_ESP}/dev/vdb c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n");
    fake.runner.respond("lsblk -rnpo NAME,PARTTYPE", 0, &two);
    fake.runner.respond(
        "blkid -c /dev/null -l -o device -t PARTUUID=0fc63daf-8483-4772-8e79-3d69d8477de4",
        0,
        "/dev/vdb\n",
    );
    let mut variable = vec![7, 0, 0, 0];
    for unit in "0fc63daf-8483-4772-8e79-3d69d8477de4"
        .encode_utf16()
        .chain([0])
    {
        variable.extend(unit.to_le_bytes());
    }
    fake.files.dir("/sys/firmware/efi").file(
        "/sys/firmware/efi/efivars/LoaderDevicePartUUID-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f",
        variable,
    );
    assert_eq!(
        mount_esp(&fake.system(), None).unwrap(),
        Path::new("/mnt/vdb")
    );

    // Two, and none named: which to mount is the person's to say.
    let fake = machine();
    fake.runner.respond("lsblk -rnpo NAME,PARTTYPE", 0, &two);
    let error = mount_esp(&fake.system(), None).unwrap_err();
    assert!(
        matches!(&error, EspError::Ambiguous(all) if all == "/dev/vda1, /dev/vdb"),
        "{error:?}"
    );
    assert!(
        error.to_string().ends_with("name the one to mount"),
        "{error}"
    );

    let none = machine();
    assert!(matches!(
        mount_esp(&none.system(), None),
        Err(EspError::NoEsp)
    ));
    assert!(
        none.runner
            .lines()
            .iter()
            .all(|line| !line.starts_with("mount "))
    );
}

#[test]
fn a_device_mounted_already_is_given_where_it_is() {
    let fake = machine();
    fake.files
        .file(MOUNTS, format!("{MOUNTED}/dev/vdb /boot/efi vfat rw 0 0\n"))
        .symlink("/dev/disk/by-id/virtio-esp", "/dev/vdb");

    for name in ["/dev/vdb", "/dev/disk/by-id/virtio-esp"] {
        assert_eq!(
            mount_esp(&fake.system(), Some(&device(name))).unwrap(),
            Path::new("/boot/efi"),
            "{name}"
        );
    }
    assert_eq!(fake.runner.lines(), Vec::<String>::new());

    // A table that cannot be read says nothing is mounted.
    let fake = machine();
    fake.files.fail(MOUNTS, io::ErrorKind::PermissionDenied);
    assert_eq!(
        mount_esp(&fake.system(), Some(&device("/dev/vdb"))).unwrap(),
        Path::new("/mnt/vdb")
    );
}

#[test]
fn a_busy_mount_point_is_refused() {
    let fake = machine();
    fake.files
        .file(MOUNTS, format!("{MOUNTED}/dev/vdc /mnt/vdb ext4 rw 0 0\n"));

    let error = mount_esp(&fake.system(), Some(&device("/dev/vdb"))).unwrap_err();

    assert!(
        matches!(&error, EspError::Busy(point) if point == Path::new("/mnt/vdb")),
        "{error:?}"
    );
    assert_eq!(error.to_string(), "mount point /mnt/vdb already in use");
    assert_eq!(fake.runner.lines(), Vec::<String>::new());
}

#[test]
fn what_is_not_a_block_device_is_refused() {
    let fake = machine();
    fake.files.file("/dev/notes", "text");

    for name in ["/dev/notes", "/dev/missing", "/dev"] {
        let error = mount_esp(&fake.system(), Some(&device(name))).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{name} does not exist or is not a block device")
        );
    }
    assert_eq!(fake.runner.lines(), Vec::<String>::new());
    assert!(!fake.files.exists(Path::new("/mnt")));
}

#[test]
fn the_mount_is_left_mounted() {
    let fake = machine();

    let point = mount_esp(&fake.system(), Some(&device("/dev/vdb"))).unwrap();

    assert_eq!(point, Path::new("/mnt/vdb"));
    assert!(fake.files.is_dir(&point));
    // Mounted, and not unmounted when the guard went.
    assert_eq!(fake.runner.lines(), ["mount /dev/vdb /mnt/vdb"]);
}

#[test]
fn a_mount_that_fails_says_where_and_how() {
    let fake = machine();
    fake.runner.respond("mount /dev/vdb /mnt/vdb", 32, "");

    let error = mount_esp(&fake.system(), Some(&device("/dev/vdb"))).unwrap_err();

    assert_eq!(
        error.to_string(),
        "unable to mount /dev/vdb at /mnt/vdb (mount exited with 32)"
    );

    let fake = machine();
    fake.files
        .fail("/mnt/vdb", io::ErrorKind::ReadOnlyFilesystem);
    let error = mount_esp(&fake.system(), Some(&device("/dev/vdb"))).unwrap_err();
    assert!(
        matches!(&error, EspError::Directory { path, .. } if path == Path::new("/mnt/vdb")),
        "{error:?}"
    );
    // Why is the cause the person sees.
    assert_eq!(
        zbm_types::code::chain(&error),
        "unable to make the mount point /mnt/vdb: Read-only file system (os error 30)"
    );
    assert_eq!(fake.runner.lines(), Vec::<String>::new());
}
