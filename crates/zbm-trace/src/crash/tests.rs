//! Tests of crash reports: they read back as written, whatever the reason.

use pretty_assertions::assert_eq;

use super::*;

fn report(reason: Reason) -> Report {
    Report {
        program: "zfsbootmenu-init".to_owned(),
        binary: "/usr/libexec/zfsbootmenu/zbm".to_owned(),
        pid: 412,
        time: 1_700_000_000,
        reason,
        build_id: Some("5b60ff289ad4ec89".to_owned()),
        spans: vec!["startup".to_owned(), "import{pool=tank}".to_owned()],
        frames: vec![
            Frame {
                offset: 0x41234,
                module: "/usr/libexec/zfsbootmenu/zbm".to_owned(),
            },
            Frame {
                offset: 0x28010,
                module: "/lib/libc.so.6".to_owned(),
            },
        ],
    }
}

#[test]
fn a_panic_report_reads_as_text() {
    let text = report(Reason::Panic {
        message: "the len is 0".to_owned(),
        location: Some("crates/zbm-boot/src/import.rs:40:9".to_owned()),
    })
    .render();

    assert_eq!(
        text,
        "ZFSBootMenu crash report\n\
         program: zfsbootmenu-init\n\
         binary: /usr/libexec/zfsbootmenu/zbm\n\
         pid: 412\n\
         time: 1700000000\n\
         reason: panicked at crates/zbm-boot/src/import.rs:40:9: the len is 0\n\
         build-id: 5b60ff289ad4ec89\n\
         spans: startup > import{pool=tank}\n\
         frames:\n\
         \x20 #0 0x0000000000041234 /usr/libexec/zfsbootmenu/zbm\n\
         \x20 #1 0x0000000000028010 /lib/libc.so.6\n"
    );
}

#[test]
fn reports_read_back_as_written() {
    let reasons = [
        Reason::Panic {
            message: "a: b: c".to_owned(),
            location: Some("src/x.rs:1:2".to_owned()),
        },
        Reason::Panic {
            message: "no location: here".to_owned(),
            location: None,
        },
        Reason::Signal {
            number: 11,
            core: true,
        },
        Reason::Signal {
            number: 99,
            core: false,
        },
    ];

    for reason in reasons {
        let mut written = report(reason);
        assert_eq!(Report::parse(&written.render()), Some(written.clone()));
        written.build_id = None;
        written.spans.clear();
        written.frames.clear();
        assert_eq!(Report::parse(&written.render()), Some(written));
    }
}

#[test]
fn signals_are_named() {
    let text = report(Reason::Signal {
        number: 11,
        core: true,
    })
    .render();

    assert!(text.contains("reason: killed by signal 11 (SIGSEGV) (core dumped)\n"));
    let names: Vec<&str> = [1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 13, 14, 15, 31, 64]
        .into_iter()
        .map(signal_name)
        .collect();
    assert_eq!(
        names,
        [
            "SIGHUP",
            "SIGINT",
            "SIGQUIT",
            "SIGILL",
            "SIGTRAP",
            "SIGABRT",
            "SIGBUS",
            "SIGFPE",
            "SIGKILL",
            "SIGSEGV",
            "SIGPIPE",
            "SIGALRM",
            "SIGTERM",
            "SIGSYS",
            "unknown signal"
        ]
    );
}

#[test]
fn other_text_is_not_a_report() {
    assert_eq!(Report::parse(""), None);
    assert_eq!(Report::parse("hello\n"), None);
    assert_eq!(
        Report::parse("ZFSBootMenu crash report\nprogram: x\npid: no\n"),
        None
    );
    assert_eq!(
        Report::parse(
            "ZFSBootMenu crash report\nprogram: x\nbinary: /x\npid: 1\ntime: 2\nreason: odd\n"
        ),
        None
    );
}

#[test]
fn file_names_sort_by_time() {
    let mut early = report(Reason::Signal {
        number: 9,
        core: false,
    });
    early.time = 1_700_000_000;
    let mut late = early.clone();
    late.time = 1_800_000_000;
    late.pid = 1;

    assert_eq!(early.file_name(), "crash-1700000000-412.txt");
    assert!(early.file_name() < late.file_name());
}
