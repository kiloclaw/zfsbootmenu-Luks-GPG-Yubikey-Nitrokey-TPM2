//! Tests of reading the kernel log back, with a fake `dmesg`.
//!
//! They are integration tests because the fakes of `zbm-testkit` implement
//! the traits of this crate as other crates see it.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    reason = "integration tests set up real files and fail loudly when they cannot"
)]

use std::path::Path;

use zbm_platform::Files as _;
use zbm_platform::dmesg::{
    Record, device_lines, filter_raw, filtered_command, format_records, is_about, kernel_log,
    levels, names, raw_prefixes, read_after, read_logs, read_logs_with_dmesg, records,
    strip_timestamps,
};
use zbm_testkit::legacy::{Bash, LOG_STUBS};
use zbm_testkit::{FakeRunner, TempDir, parity};
use zbm_trace::kmsg::Message;

#[test]
fn filtered_command_splits_the_time() {
    assert_eq!(
        filtered_command("err,warn", None).display(),
        "dmesg -f user --color=never -l err,warn"
    );
    assert_eq!(
        filtered_command("info", Some("2024-01-02 10:00")).display(),
        "dmesg -f user --color=never -l info --since 2024-01-02 10:00"
    );
    assert_eq!(filtered_command("info", Some("")).get_args().len(), 5);
}

#[test]
fn timestamps_are_removed_up_to_the_last_bracket() {
    assert_eq!(
        strip_timestamps("[    1.5] ZFSBootMenu: a\nplain\n[x] [y] b\n[no end\n"),
        "ZFSBootMenu: a\nplain\nb\n[no end\n"
    );
}

#[test]
fn raw_log_is_filtered_by_level_prefix() {
    let raw = "<3>[    1.0] ZFSBootMenu: broken\n<11>[ 2.0] ZFSBootMenu: busybox\n\
               <6>[ 3.0] ZFSBootMenu: info\n<7>[ 4.0] ZBM:[1]||debug\n\
               <3>[ 5.0] kernel: other\n";
    assert_eq!(
        filter_raw(raw, "err"),
        "ZFSBootMenu: broken \nZFSBootMenu: busybox \n"
    );
    assert_eq!(filter_raw(raw, "debug"), "ZBM:[1]||debug \n");
    assert_eq!(filter_raw(raw, "bogus").lines().count(), 4);
    assert_eq!(
        filter_raw("<3>[ 1] ZFSBootMenu: and ZBM: both\n", "err"),
        "ZFSBootMenu: and ZBM: both \nZFSBootMenu: and ZBM: both \n"
    );
    assert_eq!(raw_prefixes("err,"), ["<11>", "<3>"]);
    assert_eq!(
        raw_prefixes("err,,warn"),
        ["<11>", "<3>", "", "<12>", "<4>"]
    );
}

#[test]
fn read_logs_prefers_filtered_dmesg() {
    let runner = FakeRunner::new();
    runner.respond(
        "dmesg -f user --color=never -l err",
        0,
        "[  1.0] ZFSBootMenu: one\n\n",
    );
    assert_eq!(
        read_logs_with_dmesg(&runner, "err", None).unwrap(),
        "ZFSBootMenu: one\n"
    );
    assert_eq!(runner.lines(), ["dmesg -f user --color=never -l err"]);
    assert_eq!(
        read_logs_with_dmesg(&runner, "", None),
        Err(Message::error("levels is undefined"))
    );
}

#[test]
fn read_logs_falls_back_to_the_raw_log() {
    let runner = FakeRunner::new();
    runner.respond("dmesg -f user --color=never -l warn", 1, "");
    runner.respond("dmesg -r", 0, "<4>[ 1] ZFSBootMenu: careful\n<6>[ 2] x\n");
    assert_eq!(
        read_logs_with_dmesg(&runner, "warn", None).unwrap(),
        "ZFSBootMenu: careful \n"
    );
}

/// Runs `print_kmsg_logs` with a fake `dmesg` that prints `raw` for `dmesg -r`
/// and `filtered` (or fails when it is `None`) for the filtering call.
fn legacy_read(levels: &str, since: &str, filtered: Option<&str>, raw: &str) -> String {
    let dir = TempDir::new("zbm-log-parity");
    dir.write("filtered", filtered.unwrap_or_default());
    dir.write("raw", raw);
    let script = format!(
        "#!/bin/sh\necho \"$*\" >> {calls}\n\
         if [ \"$1\" = -r ]; then cat {raw}; exit 0; fi\n{filtered}\n",
        calls = dir.join("calls").display(),
        raw = dir.join("raw").display(),
        filtered = if filtered.is_some() {
            format!("cat {}", dir.join("filtered").display())
        } else {
            "exit 1".to_owned()
        },
    );
    dir.write_executable("bin/dmesg", script);
    let path = format!("PATH={}:\"$PATH\"", dir.join("bin").display());
    let output = Bash::new()
        .code(&path)
        .code(LOG_STUBS)
        .code("FORCE_RELOAD=1")
        .source("zfsbootmenu/lib/kmsg-log-lib.sh")
        .code(LOG_STUBS)
        .call("print_kmsg_logs", &[levels, since], b"");
    assert!(output.status.is_success(), "{}", output.stderr_text());
    output.stdout_text()
}

#[test]
fn read_logs_matches_print_kmsg_logs() {
    let raw = "<3>[    1.0] ZFSBootMenu: broken\n<11>[ 2.0] ZFSBootMenu: busybox\n\
               <6>[ 3.0] ZFSBootMenu: info here\n<7>[ 4.0] ZBM:[1]|t|debug\n\
               <3>[ 5.0] kernel: other\n<5>ZFSBootMenu: short\n\n";
    let filtered = "[    1.0] ZFSBootMenu: broken\n[ 3.0] ZFSBootMenu: info\n";
    let cases = [
        ("err", "", Some(filtered)),
        ("err,warn", "2024-01-02 10:00", Some(filtered)),
        ("info", "", Some("")),
        ("err", "", None),
        ("err,warn,notice,info,debug", "", None),
        ("debug,", "", None),
        ("unknown", "", None),
    ];
    let mut parity = parity!("read_logs_matches_print_kmsg_logs");
    for (levels, since, filtered) in cases {
        let runner = FakeRunner::new();
        let since_arg = (!since.is_empty()).then_some(since);
        let filtered_line = filtered_command(levels, since_arg).display();
        match filtered {
            Some(text) => runner.respond(&filtered_line, 0, text),
            None => runner.respond(&filtered_line, 1, ""),
        }
        runner.respond("dmesg -r", 0, raw);
        let rust = read_logs_with_dmesg(&runner, levels, since_arg).unwrap();
        let input = format!("levels {levels:?}, since {since:?}, filtered {filtered:?}");
        let legacy: String = parity.legacy(input, || legacy_read(levels, since, filtered, raw));
        assert_eq!(rust, legacy, "levels {levels:?}, since {since:?}");
    }
}

/// Records of `/dev/kmsg`: the kernel's, ZFSBootMenu's at three levels, a
/// dictionary line, an escaped character, and a line that is no record.
const KMSG: &str = "6,1,921824,-;ZFSBootMenu: setting log level\n\
    3,2,1093623,-;virtio-pci 0000:00:05.0: vgaarb\n\
    \x20SUBSYSTEM=pci\n\
    \x20DEVICE=+pci:0000:00:05.0\n\
    11,3,2000001,-;ZFSBootMenu: broken\\x09pool\n\
    12,4,2500000,-;ZFSBootMenu: careful\n\
    14,5,3000000,c;ZFSBootMenu: info\n\
    no record\n";

#[test]
fn records_are_read_from_kmsg_with_their_escapes_undone() {
    let read = records(KMSG.as_bytes());
    assert_eq!(read.len(), 5);
    assert_eq!(
        read[2],
        Record {
            facility: 1,
            level: 3,
            sequence: 3,
            micros: 2_000_001,
            text: "ZFSBootMenu: broken\tpool".to_owned(),
            device: None,
        }
    );
    assert_eq!(
        format_records(&read[..2]),
        "[    0.921824] ZFSBootMenu: setting log level\n\
         [    1.093623] virtio-pci 0000:00:05.0: vgaarb\n"
    );
}

#[test]
fn the_log_comes_from_kmsg_and_from_dmesg_without_it() {
    let unreadable = zbm_testkit::fakes::FakeSystem::new();
    unreadable
        .files
        .fail("/dev/kmsg", std::io::ErrorKind::PermissionDenied);
    unreadable.runner.respond("dmesg", 0, "[ 1.0] from dmesg\n");
    assert_eq!(
        kernel_log(&unreadable.files, &unreadable.runner),
        "[ 1.0] from dmesg\n"
    );
    // Written by ZFSBootMenu, read back as the kernel's records.
    let fake = zbm_testkit::fakes::FakeSystem::new();
    fake.files
        .append(Path::new("/dev/kmsg"), b"<3>ZFSBootMenu: written\n")
        .unwrap();
    assert_eq!(
        read_logs(&fake.system(), "err").unwrap(),
        "ZFSBootMenu: written\n"
    );
    fake.files.file("/dev/kmsg", KMSG);
    let log = kernel_log(&fake.files, &fake.runner);
    assert!(
        log.starts_with("[    0.921824] ZFSBootMenu: setting"),
        "{log}"
    );
    assert!(!log.contains("SUBSYSTEM"), "{log}");
    // User messages of the levels asked for, as `dmesg -f user -l` prints
    // them without their timestamps: not the kernel's error.
    assert_eq!(
        read_logs(&fake.system(), "err,warn").unwrap(),
        "ZFSBootMenu: broken\tpool\nZFSBootMenu: careful\n"
    );
    assert_eq!(read_logs(&fake.system(), "debug").unwrap(), "\n");
    assert!(read_logs(&fake.system(), "").is_err());
}

#[test]
fn a_record_keeps_its_sequence_number() {
    let numbers: Vec<u64> = records(KMSG.as_bytes())
        .iter()
        .map(|record| record.sequence)
        .collect();
    assert_eq!(numbers, [1, 2, 3, 4, 5]);
    // A record whose number is no number is no record.
    assert!(records(b"3,x,1,-;ZFSBootMenu: broken\n").is_empty());
}

#[test]
fn records_after_a_number_are_the_newer_ones_and_the_last_number_is_returned() {
    let fake = zbm_testkit::fakes::FakeSystem::new();
    fake.files.file("/dev/kmsg", KMSG);
    let texts = |read: &[Record]| -> Vec<String> {
        read.iter().map(|record| record.text.clone()).collect()
    };
    // ZFSBootMenu's errors and warnings, not the kernel's error; the last
    // number is that of the last record, whatever its level.
    let (all, last) = read_after(&fake.files, &[3, 4], None).unwrap();
    assert_eq!(
        texts(&all),
        ["ZFSBootMenu: broken\tpool", "ZFSBootMenu: careful"]
    );
    assert_eq!(last, Some(5));
    let (newer, last) = read_after(&fake.files, &[3, 4], Some(3)).unwrap();
    assert_eq!(texts(&newer), ["ZFSBootMenu: careful"]);
    assert_eq!(last, Some(5));
    let (none, last) = read_after(&fake.files, &[3, 4], Some(5)).unwrap();
    assert!(none.is_empty());
    assert_eq!(last, Some(5));

    fake.files.file("/dev/kmsg", "");
    assert_eq!(read_after(&fake.files, &[3], None).unwrap(), (vec![], None));
    fake.files
        .fail("/dev/kmsg", std::io::ErrorKind::PermissionDenied);
    assert!(read_after(&fake.files, &[3], None).is_err());
}

#[test]
fn an_unknown_level_name_is_none() {
    assert_eq!(levels("err,warn"), Some(vec![3, 4]));
    assert_eq!(levels("debug,emerg,err,3"), Some(vec![0, 3, 7]));
    assert_eq!(levels("err,bogus"), None);
    assert_eq!(levels("error"), None);
    assert_eq!(levels(" err"), None);
    // An empty name between commas is none, as `dmesg -l` says; an empty
    // list selects no level.
    assert_eq!(levels("err,"), None);
    assert_eq!(levels(","), None);
    assert_eq!(levels(""), Some(vec![]));
}

#[test]
fn levels_are_named_as_dmesg_takes_them() {
    // Any case, and numbers from 0 to 7.
    assert_eq!(levels("ERR,Warn"), Some(vec![3, 4]));
    assert_eq!(levels("0,7,03"), Some(vec![0, 3, 7]));
    assert_eq!(levels("8"), None);
    assert_eq!(levels("99999999999999999999999"), None);
    // `+` after a level adds the graver ones, before it the lighter ones.
    assert_eq!(levels("err+"), Some(vec![0, 1, 2, 3]));
    assert_eq!(levels("+notice"), Some(vec![5, 6, 7]));
    assert_eq!(levels("+6,crit"), Some(vec![2, 6, 7]));
    assert_eq!(levels("+err+"), None);
    assert_eq!(levels("+"), None);
}

/// The kernel's records about `sdb` and its partition `sdb2`: one tagged
/// with the disk's numbers and naming none, one naming the partition with
/// an escape character in it, one naming the port alone, and those of
/// other disks.
const DEVICES: &str = "3,10,5000000,-;I/O error, dev sdb, sector 2048\n\
    \x20SUBSYSTEM=block\n\
    \x20DEVICE=b8:16\n\
    4,11,5100000,-;Buffer I/O error on device\n\
    \x20DEVICE=b8:16\n\
    6,12,5200000,-;EXT4-fs (sdb2): \\x1b[31mred\\x1b[0m\n\
    3,13,5300000,-;ata3.00: failed command: READ FPDMA QUEUED\n\
    6,14,5400000,-;sd 2:0:0:0: [sdb1] attached\n\
    6,15,5500000,-;nvme0n1p2: unknown partition table\n\
    6,16,5600000,-;tagged elsewhere\n\
    \x20DEVICE=b8:0\n\
    no record\n\
    \x20DEVICE=b8:16\n";

#[test]
fn a_devices_lines_are_those_naming_it_as_a_word_or_tagged_with_its_numbers() {
    let read = records(DEVICES.as_bytes());
    let devices: Vec<Option<&str>> = read.iter().map(|record| record.device.as_deref()).collect();
    // The dictionary of a line that is no record is no record's.
    assert_eq!(
        devices,
        [
            Some("b8:16"),
            Some("b8:16"),
            None,
            None,
            None,
            None,
            Some("b8:0")
        ]
    );
    let about: Vec<&str> = read
        .iter()
        .filter(|record| is_about(record, &["sdb2", "sdb"], &["8:18", "8:16"]))
        .map(|record| record.text.as_str())
        .collect();
    // The ESC byte is the kernel's text, which whoever draws it makes
    // harmless; `ata3.00` names a port, not the disk.
    assert_eq!(
        about,
        [
            "I/O error, dev sdb, sector 2048",
            "Buffer I/O error on device",
            "EXT4-fs (sdb2): \u{1b}[31mred\u{1b}[0m",
        ]
    );
    for (text, name, named) in [
        ("[sda] ready", "sda", true),
        ("sda: sda1 sda2", "sda", true),
        ("sda1: ready", "sda", false),
        ("nvme0n1p2 ready", "nvme0n1", false),
        ("of dm-0.", "dm-0", true),
        ("dm-01", "dm-0", false),
        ("ésda", "sda", false),
        ("anything", "", false),
    ] {
        assert_eq!(names(text, name), named, "{text:?} {name:?}");
    }
}

#[test]
fn a_devices_lines_come_from_kmsg_and_from_dmesg_by_name_without_it() {
    let fake = zbm_testkit::fakes::FakeSystem::new();
    fake.files.file("/dev/kmsg", DEVICES);
    assert_eq!(
        device_lines(&fake.files, &fake.runner, &["sdb1"], &["8:17"]),
        ["[    5.400000] sd 2:0:0:0: [sdb1] attached"]
    );
    assert!(device_lines(&fake.files, &fake.runner, &["sdc"], &["8:32"]).is_empty());

    fake.files
        .fail("/dev/kmsg", std::io::ErrorKind::PermissionDenied);
    fake.runner.respond(
        "dmesg",
        0,
        "[ 5.0] I/O error, dev sdb, sector 2048\n[ 5.1] sdb1: ready\n",
    );
    assert_eq!(
        device_lines(&fake.files, &fake.runner, &["sdb"], &["8:16"]),
        ["[ 5.0] I/O error, dev sdb, sector 2048"]
    );
}
