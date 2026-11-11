use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeFiles;

use zbm_tui::menu::keymap::Map;
use zbm_tui::menu::pager::Pager;

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

/// `ztrace` on the kernel log `kmsg`, its lines printed as they are
/// without a terminal: its exit status, standard output and standard
/// error.
fn trace(kmsg: Option<&str>) -> (i32, String, String) {
    trace_with(kmsg, &mut crate::help::printed)
}

/// `ztrace` on the kernel log `kmsg`, its lines shown with `shown_with`:
/// its exit status, standard output and standard error.
fn trace_with(kmsg: Option<&str>, shown_with: crate::help::Show<'_>) -> (i32, String, String) {
    let files = FakeFiles::new();
    match kmsg {
        Some(kmsg) => {
            files.file("/dev/kmsg", kmsg);
        }
        None => {
            files.fail("/dev/kmsg", std::io::ErrorKind::PermissionDenied);
        }
    }
    let mut paged = Paged {
        map: Map::default(),
        show: shown_with,
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let status = show(
        &files,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
        &mut paged,
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
        trace(Some(KMSG)),
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
    // On a terminal too, the calls are not coloured, where the legacy
    // `ztrace` painted them: the pager would show the colours' escapes.
    let mut drawn = Vec::new();
    let mut draw = |pager: &Pager, _: Map, _: &mut dyn std::io::Write| {
        drawn.extend(pager.lines.iter().cloned());
        Ok(())
    };
    assert_eq!(trace_with(Some(KMSG), &mut draw).0, 0);
    let printed: Vec<String> = trace(Some(KMSG)).1.lines().map(str::to_owned).collect();
    assert_eq!(drawn, printed);
    assert!(drawn.iter().all(|line| !line.contains('\x1b')));
}

#[test]
fn trace_without_debug_lines_says_why() {
    let warning = "12,1,0,-;ZFSBootMenu: careful\n";
    assert_eq!(trace(Some(warning)), (0, NONE.to_owned(), String::new()));
    assert!(NONE.contains("loglevel=7"));
    let (status, stdout, stderr) = trace(None);
    assert_eq!((status, stdout.as_str()), (1, ""));
    assert!(
        stderr.starts_with("logs: unable to read the kernel log:"),
        "{stderr}"
    );
}
