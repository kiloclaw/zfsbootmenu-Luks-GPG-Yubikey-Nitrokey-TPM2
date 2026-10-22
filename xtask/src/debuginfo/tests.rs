//! Tests of crash report symbolization: reading reports and `addr2line`.

use super::*;

const REPORT: &str = "\
ZFSBootMenu crash report
program: debug
binary: /usr/libexec/zfsbootmenu/zbm
pid: 412
time: 1700000000
reason: panicked at crates/zbm/src/debug.rs:88:5: boom
build-id: 5b60ff28
spans: crash_test{reason=zbm debug panic}
frames:
  #0 0x0000000000041234 /usr/libexec/zfsbootmenu/zbm
  #1 0x0000000000028010 /lib/libc.so.6
  #2 0x0000000000041300 /usr/libexec/zfsbootmenu/zbm
";

#[test]
fn reports_are_read_with_their_frames() {
    let report = parse_report(REPORT).unwrap();

    assert_eq!(report.binary, "/usr/libexec/zfsbootmenu/zbm");
    assert_eq!(report.build_id.as_deref(), Some("5b60ff28"));
    assert_eq!(report.frames.len(), 3);
    assert_eq!(report.frames[1], (1, 0x28010, "/lib/libc.so.6".to_owned()));
    assert_eq!(report.header.len(), 8);
    assert!(parse_report("hello").is_err());
}

#[test]
fn addr2line_output_is_grouped_by_address() {
    let output = "\
0x0000000000041233: zbm::debug::panic_for_test at /src/crates/zbm/src/debug.rs:88
 (inlined by) zbm::debug::run_in at /src/crates/zbm/src/debug.rs:60
0x00000000000412ff: main at /src/crates/zbm/src/main.rs:30
";

    let groups = parse_addr2line(output);

    assert_eq!(
        groups,
        [
            vec![
                "zbm::debug::panic_for_test at /src/crates/zbm/src/debug.rs:88".to_owned(),
                "(inlined by) zbm::debug::run_in at /src/crates/zbm/src/debug.rs:60".to_owned(),
            ],
            vec!["main at /src/crates/zbm/src/main.rs:30".to_owned()],
        ]
    );
}

#[test]
fn frames_of_other_binaries_stay_unresolved() {
    let report = parse_report(REPORT).unwrap();
    let resolved = vec![
        vec![
            "f at a.rs:1".to_owned(),
            "(inlined by) g at a.rs:2".to_owned(),
        ],
        vec!["main at main.rs:3".to_owned()],
    ];

    let text = render(&report, &resolved);

    assert!(text.ends_with(
        "frames:\n\
         \x20 #0   0x00041234 f at a.rs:1\n\
         \x20        (inlined by) g at a.rs:2\n\
         \x20 #1   0x00028010 in /lib/libc.so.6\n\
         \x20 #2   0x00041300 main at main.rs:3\n"
    ));
    assert!(text.starts_with("ZFSBootMenu crash report\nprogram: debug\n"));
}

/// A function whose address the end-to-end test looks up.
#[inline(never)]
fn marker() -> u32 {
    std::hint::black_box(7)
}

/// Whether `program` runs, so that tests that need binutils can skip.
fn have(program: &str) -> bool {
    std::process::Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn a_report_of_this_binary_is_symbolized_from_its_split_debug_file() {
    if !have("objcopy") || !have("addr2line") {
        eprintln!("skipped: objcopy and addr2line (binutils) are needed");
        return;
    }
    let root = crate::workspace_root();
    let dir = std::env::temp_dir().join(format!("xtask-symbolize-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let exe = std::fs::canonicalize("/proc/self/exe").unwrap();
    let copy = dir.join("binary");
    std::fs::copy(&exe, &copy).unwrap();
    // Where `marker` is, as an offset from this binary's load address.
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    let base = maps
        .lines()
        .filter(|line| line.ends_with(&*exe.to_string_lossy()))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let start = fields.next()?.split('-').next()?;
            let offset = fields.nth(1)?;
            let start = u64::from_str_radix(start, 16).ok()?;
            Some(start - u64::from_str_radix(offset, 16).ok()?)
        })
        .min()
        .unwrap();
    let function: fn() -> u32 = marker;
    let offset = function as usize as u64 + 1 - base;

    split(&root, &copy, &dir.join("debug")).unwrap();
    let id = elf::build_id(&std::fs::read(&copy).unwrap()).unwrap();
    let report = dir.join("report.txt");
    std::fs::write(
        &report,
        format!(
            "ZFSBootMenu crash report\nprogram: xtask\nbinary: {path}\npid: 1\ntime: 2\n\
             reason: panicked: test\nbuild-id: {id}\nspans: (none)\nframes:\n\
             \x20 #0 {offset:#018x} {path}\n",
            path = exe.display()
        ),
    )
    .unwrap();
    let text = symbolized(&root, &report, &[dir.join("debug")]).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(marker(), 7);
    assert!(std::fs::metadata(&copy).is_err());
    assert!(text.contains("debuginfo::tests::marker at "), "{text}");
    assert!(text.contains("xtask/src/debuginfo/tests.rs:"), "{text}");
}

#[test]
fn a_report_without_a_build_id_cannot_be_symbolized() {
    let dir = std::env::temp_dir().join(format!("xtask-no-id-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let report = dir.join("report.txt");
    std::fs::write(
        &report,
        "ZFSBootMenu crash report\nbuild-id: unknown\nframes:\n",
    )
    .unwrap();

    let error = symbolized(Path::new("."), &report, std::slice::from_ref(&dir)).unwrap_err();
    let missing = symbolized(Path::new("."), &dir.join("missing"), &[]).unwrap_err();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(error.to_string(), "the report names no build ID");
    assert!(missing.to_string().starts_with("reading "));
}

#[test]
fn a_binary_without_a_build_id_is_not_split() {
    let dir = std::env::temp_dir().join(format!("xtask-split-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("script");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();

    let error = split(Path::new("."), &script, &dir).unwrap_err();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        error.to_string().ends_with("has no GNU build ID"),
        "{error}"
    );
}

#[test]
fn a_missing_debug_file_says_where_it_looked() {
    let error = find_debug_file(&[PathBuf::from("/nonexistent")], "abcd").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("no debug file abcd.debug in /nonexistent")
    );
}
