//! Tests of the stack of a device, on a fake `/sys/class/block`.

use pretty_assertions::assert_eq;
use zbm_testkit::FakeRunner;
use zbm_testkit::fakes::FakeFiles;

use super::*;

/// The kernel's record of the disk `sda` with its partitions, a LUKS
/// mapping `dm-0` on `sda2`, and the node of each in `/dev`, with a name
/// of `sda2` by its identity.
fn sysfs() -> FakeFiles {
    let files = FakeFiles::new();
    for (name, dev) in [("sda", "8:0"), ("dm-0", "253:0")] {
        files.file(format!("/sys/class/block/{name}/dev"), format!("{dev}\n"));
        files.block_device(format!("/dev/{name}"));
    }
    for (name, dev) in [("sda1", "8:1"), ("sda2", "8:2")] {
        for dir in [
            format!("/sys/class/block/{name}"),
            format!("/sys/class/block/sda/{name}"),
        ] {
            files.file(format!("{dir}/dev"), format!("{dev}\n"));
            files.file(format!("{dir}/partition"), "1\n");
        }
        files.block_device(format!("/dev/{name}"));
    }
    files.file("/sys/class/block/dm-0/slaves/sda2", "");
    files.symlink("/dev/mapper/luks-a", "/dev/dm-0");
    files.symlink("/dev/disk/by-id/ata-disk-part2", "/dev/sda2");
    files
}

/// The names and numbers of the stack of `device`.
fn stack(files: &FakeFiles, device: &str) -> Vec<(String, Option<String>)> {
    let runner = FakeRunner::new();
    Devices::new(&runner, files)
        .stack(device)
        .into_iter()
        .map(|found| (found.name, found.numbers))
        .collect()
}

fn found(name: &str, numbers: Option<&str>) -> (String, Option<String>) {
    (name.to_owned(), numbers.map(str::to_owned))
}

#[test]
fn a_partition_stands_on_its_disk_and_a_mapping_on_its_slaves() {
    let files = sysfs();

    assert_eq!(
        stack(&files, "/dev/disk/by-id/ata-disk-part2"),
        [found("sda2", Some("8:2")), found("sda", Some("8:0"))]
    );
    assert_eq!(
        stack(&files, "/dev/mapper/luks-a"),
        [
            found("dm-0", Some("253:0")),
            found("sda2", Some("8:2")),
            found("sda", Some("8:0")),
        ]
    );
    // A pool not imported names its devices as `/dev` does.
    assert_eq!(stack(&files, "sda"), [found("sda", Some("8:0"))]);
}

#[test]
fn a_pool_not_imported_names_its_devices_in_the_directories_zpool_searches() {
    let files = sysfs();
    files.symlink("/dev/disk/by-id/virtio-d0", "/dev/sda");

    // `zpool import` prints `virtio-d0` for `/dev/disk/by-id/virtio-d0`,
    // and the name of a mapping without `/dev/mapper`.
    assert_eq!(stack(&files, "virtio-d0"), [found("sda", Some("8:0"))]);
    assert_eq!(stack(&files, "luks-a")[0], found("dm-0", Some("253:0")));
    assert_eq!(stack(&files, "sda2")[0], found("sda2", Some("8:2")));
}

#[test]
fn a_lost_partition_is_its_name_then_its_disks() {
    let files = sysfs();
    files.file("/sys/class/block/nvme0n1/dev", "259:0\n");

    // A pool keeps the path of a device it lost; the kernel names its
    // disk in the messages of why.
    assert_eq!(
        stack(&files, "/dev/sdb2"),
        [found("sdb2", None), found("sdb", None)]
    );
    assert_eq!(stack(&files, "sdb"), [found("sdb", None)]);
    // The disk is still listed: what it stands on with it.
    assert_eq!(
        stack(&files, "/dev/sda9"),
        [found("sda9", None), found("sda", Some("8:0"))]
    );
    assert_eq!(
        stack(&files, "/dev/nvme0n1p3"),
        [found("nvme0n1p3", None), found("nvme0n1", Some("259:0"))]
    );
    assert_eq!(
        stack(&files, "mmcblk1p2"),
        [found("mmcblk1p2", None), found("mmcblk1", None)]
    );
    assert_eq!(stack(&files, "/dev/dm-7"), [found("dm-7", None)]);
}

#[test]
fn other_names_and_paths_are_nothing() {
    let files = sysfs();
    // A file of `/dev` that is no block device the kernel lists.
    files.file("/dev/null", "");
    // A file a pool is made of, whose name a disk also has.
    files.file("/images/sda", "");

    for refused in [
        // A label's name the kernel gives no disk: its word could be any
        // line's, as `error`.
        "error",
        "virtio-d0",
        "null",
        "/dev/disk/by-id/gone",
        "/images/sda",
        "/images/gone",
        "..",
        "../sys",
        "/dev/..",
        "",
        "/etc/passwd",
    ] {
        assert_eq!(stack(&files, refused), [], "{refused:?}");
    }
}

#[test]
fn only_the_kernels_names_of_disks_and_partitions_have_a_disk() {
    for (name, disk) in [
        ("sdb", "sdb"),
        ("sdb2", "sdb"),
        ("sdaa12", "sdaa"),
        ("vda1", "vda"),
        ("xvdb3", "xvdb"),
        ("hdc", "hdc"),
        ("nvme0n1", "nvme0n1"),
        ("nvme10n2p11", "nvme10n2"),
        ("mmcblk0p1", "mmcblk0"),
        ("loop0", "loop0"),
        ("loop0p1", "loop0"),
        ("md127", "md127"),
        ("zd16p2", "zd16"),
        ("dm-3", "dm-3"),
    ] {
        assert_eq!(disk_of(name), Some(disk), "{name:?}");
    }
    for name in [
        "",
        "sd",
        "sd2",
        "sdB",
        "sdb2x",
        "nvme",
        "nvme0",
        "nvme0n",
        "nvmexn1",
        "mmcblk",
        "mmcblkp1",
        "loop",
        "dm-",
        "dm-x",
        "error",
        "p1",
        "virtio-d0",
    ] {
        assert_eq!(disk_of(name), None, "{name:?}");
    }
}

#[test]
fn a_loop_in_sys_is_followed_once() {
    let files = sysfs();
    // A mapping whose slave is itself, and one on it.
    files.file("/sys/class/block/dm-1/dev", "253:1\n");
    files.file("/sys/class/block/dm-1/slaves/dm-1", "");
    files.file("/sys/class/block/dm-1/slaves/dm-0", "");
    files.block_device("/dev/dm-1");

    let names: Vec<String> = stack(&files, "/dev/dm-1")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, ["dm-1", "dm-0", "sda2", "sda"]);
}

#[test]
fn names_outside_sys_class_block_are_refused() {
    for name in ["", ".", "..", "a/b", "a\0b"] {
        assert_eq!(entry(name), None, "{name:?}");
    }
    assert_eq!(entry("dm-0"), Some("dm-0"));
}
