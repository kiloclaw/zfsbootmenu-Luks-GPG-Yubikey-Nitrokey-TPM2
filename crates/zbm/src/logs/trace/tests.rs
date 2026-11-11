use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeFiles;

use super::*;

/// The kernel log's records, as `/dev/kmsg` gives them: shell code's and
/// Rust's debug lines, a message of two lines, a warning, the kernel's own
/// debug line and one that is not ZFSBootMenu's.
const KMSG: &str = "\
15,10,1500000,-;ZBM:[42]|main,/bin/zfsbootmenu,12;load_be,/lib/core.sh,99|loading \\x1b[2Jtank
15,11,2000001,-;ZBM:[7]||first of two
15,12,2000001,-;ZBM:[7]|crates/zbm/src/a.rs,40|second of two
12,13,2000002,-;ZFSBootMenu: careful
7,14,2000003,-;ZBM: the kernel's
15,15,2000004,-;another program's
";

/// `ztrace` on the kernel log `kmsg`, in colour or not: its exit status,
/// standard output and standard error.
fn trace(kmsg: Option<&str>, colour: bool) -> (i32, String, String) {
    let files = FakeFiles::new();
    match kmsg {
        Some(kmsg) => {
            files.file("/dev/kmsg", kmsg);
        }
        None => {
            files.fail("/dev/kmsg", std::io::ErrorKind::PermissionDenied);
        }
    }
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let status = show(
        &files,
        colour,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    (
        status,
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

#[test]
fn trace_takes_the_shell_and_the_rust_shapes() {
    assert_eq!(
        trace(Some(KMSG), false),
        (
            0,
            "\
[    1.500000]   ZBM:[42]: loading \\x1b[2Jtank
[    1.500000]      trace: main@/bin/zfsbootmenu#12
[    1.500000]      trace:  load_be@/lib/core.sh#99
[    2.000001]    ZBM:[7]: first of two
[    2.000001]    ZBM:[7]: second of two
[    2.000001]      trace: crates/zbm/src/a.rs#40
"
            .to_owned(),
            String::new()
        )
    );
    // On a terminal, the function in red, the first file in green and the
    // others in orange, as the legacy `ztrace` painted them.
    let (_, coloured, _) = trace(Some(KMSG), true);
    let lines: Vec<&str> = coloured.lines().collect();
    assert_eq!(
        lines[1],
        "[    1.500000]      trace: \x1b[0;31mmain@\x1b[0;32m/bin/zfsbootmenu\x1b[0m#12"
    );
    assert_eq!(
        lines[2],
        "[    1.500000]      trace:  \x1b[0;31mload_be@\x1b[0;33m/lib/core.sh\x1b[0m#99"
    );
    assert_eq!(
        lines[5],
        "[    2.000001]      trace: \x1b[0;32mcrates/zbm/src/a.rs\x1b[0m#40"
    );
}

#[test]
fn trace_without_debug_lines_says_why() {
    let warning = "12,1,0,-;ZFSBootMenu: careful\n";
    assert_eq!(
        trace(Some(warning), false),
        (0, NONE.to_owned(), String::new())
    );
    assert!(NONE.contains("loglevel=7"));
    let (status, stdout, stderr) = trace(None, false);
    assert_eq!((status, stdout.as_str()), (1, ""));
    assert!(
        stderr.starts_with("logs: unable to read the kernel log:"),
        "{stderr}"
    );
}
