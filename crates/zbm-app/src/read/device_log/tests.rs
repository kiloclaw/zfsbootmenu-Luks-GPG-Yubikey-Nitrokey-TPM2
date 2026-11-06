//! Tests of a device's kernel messages, on a fake `/sys/class/block` and
//! `/dev/kmsg`.

use std::fmt::Write as _;

use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// The kernel log: a message tagged with `sdb`'s numbers, one naming
/// `sdb2` with an escape character in it, one naming its port alone, and
/// one of another disk.
const KMSG: &str = "3,10,5000000,-;I/O error, dev sdb, sector 2048\n\
    \x20DEVICE=b8:16\n\
    6,11,5100000,-;EXT4-fs (sdb2): \\x1b]0;owned\\x07\n\
    3,12,5200000,-;ata3.00: failed command: READ FPDMA QUEUED\n\
    6,13,5300000,-;sd 0:0:0:0: [sda] attached\n";

/// A machine whose disk `sdb` has the partition `sdb2`, and whose kernel
/// log is [`KMSG`].
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    let files = &fake.files;
    files.file("/sys/class/block/sdb/dev", "8:16\n");
    for dir in ["/sys/class/block/sdb2", "/sys/class/block/sdb/sdb2"] {
        files.file(format!("{dir}/dev"), "8:18\n");
        files.file(format!("{dir}/partition"), "2\n");
    }
    files.block_device("/dev/sdb2");
    files.block_device("/dev/sdc");
    files.file("/sys/class/block/sdc/dev", "8:32\n");
    files.file("/dev/kmsg", KMSG);
    fake
}

fn texts(found: &[Untrusted<String>]) -> Vec<&str> {
    found.iter().map(|text| text.get().as_str()).collect()
}

#[test]
fn a_partitions_messages_include_its_disks_and_keep_the_kernels_text() {
    let fake = machine();
    let device = Untrusted::new("/dev/sdb2".to_owned());

    let log = device_log(&fake.files, &fake.runner, &device);

    assert_eq!(log.device, device);
    assert_eq!(texts(&log.names), ["sdb2", "sdb"]);
    // The ESC byte stays the kernel's: the view draws it harmlessly. The
    // port's line names no block device, and is not among them.
    assert_eq!(
        texts(&log.lines),
        [
            "[    5.000000] I/O error, dev sdb, sector 2048",
            "[    5.100000] EXT4-fs (sdb2): \u{1b}]0;owned\u{7}",
        ]
    );
    assert_eq!(
        zbm_types::Printable(log.lines[1].get()).to_string(),
        "[    5.100000] EXT4-fs (sdb2): \\x1b]0;owned\\x07"
    );
}

#[test]
fn a_device_with_no_messages_and_one_the_kernel_does_not_list_say_so() {
    let fake = machine();

    let quiet = device_log(&fake.files, &fake.runner, &Untrusted::new("sdc".to_owned()));
    assert_eq!(texts(&quiet.names), ["sdc"]);
    assert!(quiet.lines.is_empty());

    let unknown = Untrusted::new("/dev/disk/by-id/gone".to_owned());
    let unknown = device_log(&fake.files, &fake.runner, &unknown);
    assert!(unknown.names.is_empty() && unknown.lines.is_empty());
    // Nothing read: no name to look for.
    assert!(fake.runner.calls().is_empty());
}

#[test]
fn a_lost_partitions_messages_include_those_of_its_disk() {
    let fake = FakeSystem::new();
    // The kernel lists neither `sdb2` nor its disk any more.
    fake.files.file("/dev/kmsg", KMSG);
    let device = Untrusted::new("/dev/sdb2".to_owned());

    let log = device_log(&fake.files, &fake.runner, &device);

    assert_eq!(texts(&log.names), ["sdb2", "sdb"]);
    assert_eq!(log.lines.len(), 2, "{:?}", log.lines);
    assert!(
        log.lines[0]
            .get()
            .ends_with("I/O error, dev sdb, sector 2048")
    );
}

#[test]
fn a_disk_of_a_pool_not_imported_is_found_by_its_name_in_by_id() {
    let fake = machine();
    // `zpool import` prints `virtio-d0` for this link.
    fake.files.symlink("/dev/disk/by-id/virtio-d0", "/dev/sdc");
    let device = Untrusted::new("virtio-d0".to_owned());

    let log = device_log(&fake.files, &fake.runner, &device);

    assert_eq!(texts(&log.names), ["sdc"]);
    // A label's name the kernel gives no disk names nothing.
    let error = device_log(
        &fake.files,
        &fake.runner,
        &Untrusted::new("error".to_owned()),
    );
    assert!(error.names.is_empty() && error.lines.is_empty());
}

#[test]
fn only_the_newest_messages_are_kept() {
    let fake = machine();
    let mut many = String::new();
    for count in 0..LINES + 2 {
        let _ = writeln!(many, "6,{count},{count},-;sdc: message {count}");
    }
    fake.files.file("/dev/kmsg", many);

    let log = device_log(&fake.files, &fake.runner, &Untrusted::new("sdc".to_owned()));

    assert_eq!(log.lines.len(), LINES);
    assert!(
        log.lines[0].get().ends_with("sdc: message 2"),
        "{:?}",
        log.lines[0]
    );
}
