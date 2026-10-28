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
    Record, filter_raw, filtered_command, format_records, kernel_log, raw_prefixes, read_logs,
    read_logs_with_dmesg, records, strip_timestamps,
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
            micros: 2_000_001,
            text: "ZFSBootMenu: broken\tpool".to_owned(),
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
    assert_eq!(kernel_log(&unreadable.system()), "[ 1.0] from dmesg\n");
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
    let log = kernel_log(&fake.system());
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
