use pretty_assertions::assert_eq;

use super::fallback::{BACK, CELLS_LOG, KILL, LOG, STATE};
use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// A screenshot of 9 colours, one row each.
fn dump(_: &Vm<'_>, path: &Path) -> Result<()> {
    let mut bytes = b"P6\n4 9\n255\n".to_vec();
    for colour in 0..9_u8 {
        bytes.extend(std::iter::repeat_n([colour, 0, 0], 4).flatten());
    }
    fs::write(path, bytes).map_err(Error::io("writing the screenshot"))
}

/// A screenshot that never comes.
fn no_dump(_: &Vm<'_>, _: &Path) -> Result<()> {
    Ok(())
}

fn key(_: &Vm<'_>, _: &str) -> Result<()> {
    Ok(())
}

/// No pauses, and no waiting.
const NOW: Pace = Pace {
    pause: Duration::ZERO,
    patience: 0,
};

#[test]
fn text_is_typed_with_qemu_s_key_names() {
    assert_eq!(
        key_names("ls -l /a.b\n").unwrap(),
        [
            "l", "s", "spc", "minus", "l", "spc", "slash", "a", "dot", "b", "ret"
        ]
    );
    assert_eq!(
        key_names("$(x)|>:").unwrap(),
        [
            "shift-4",
            "shift-9",
            "x",
            "shift-0",
            "shift-backslash",
            "shift-dot",
            "shift-semicolon"
        ]
    );
    for text in [
        TTY, KILL, BACK, LOG, CELLS_LOG, STATE[0], STATE[1], STATE[2],
    ] {
        assert!(key_names(text).is_some(), "{text}");
    }
    assert_eq!(key_names("S").unwrap(), ["shift-s"]);
    assert_eq!(key_names("~"), None);
    assert_eq!(
        german_key_names("zy /\n").unwrap(),
        ["y", "z", "spc", "shift-7", "ret"]
    );
    assert!(german_key_names(&format!("{LAYOUT}us\n")).is_some());
    assert_eq!(german_key_names("$"), None);
    assert_eq!(german_key_names("Z"), None);
}

const LOGGED: &str = "speed 38400 baud; isig icanon\r\nSigIgn: 0000000000000000\r\n\
    zbm-state-end\r\nzbm-flood-ok\r\nzbm-back-/dev/pts/1\r\n\
    [   40.2] ZFSBootMenu: the console on the display ended (wait status 9); \
    going on on the kernel console\r\n\
    [   41.2] ZFSBootMenu: the console on the display started again (process 812)\r\n\
    [    3.1] ZFSBootMenu: console: the menu draws on its cells (client channel version 1)\r\n\
    Uid:\t60600\t60600\t60600\t60600\r\nCapEff:\t0000000004000000\r\n\
    NoNewPrivs:\t1\r\nSeccomp:\t2\r\nSeccomp_filters:\t1\r\n\
    zbmtest-denied 0 zbmtest-confined 2\r\n";

#[test]
fn the_console_shows_takes_keys_and_falls_back() {
    let settings = settings("runtime-console-run");
    let test = Test::new(&settings, "runtime-rust-console").unwrap();
    let mirror = mirror_path(&test, "virtio");
    fs::write(
        &mirror,
        "\x0czbm console: on /dev/dri/card0: 1280x800 display, DejaVuSansMono.ttf 10x20 cells, \
         128x40, keyboard us\n\
         \x0cBooting tplain/ROOT/be in 29 seconds\n\x0c [1 Boot] 2 Snapshots\n BOOT ENVIRONMENTS \n\
         \x0cemergency shell\n\x0c# tty\n/dev/pts/0\n\
         \x0c# layout de\nkeyboard: de\n\x0c# cat\nzyê\nzyê\n\
         \x0czfsbootmenu / > echo zyê\nzyê\n\x0c# layout us\nkeyboard: us\n\
         \x0czbm console: handed the display back\n\x0c# echo zbm-reattached\nzbm-reattached\n\
         \x0cZFSBootMenu ended unexpectedly; type 'exit' to start it again\n",
    )
    .unwrap();
    let (mut vm, guest) = scripted(&test, vec![Step::Say(LOGGED)]);
    let monitor = Monitor { dump, key };

    check_console(&test, &mut vm, &mirror, &monitor, NOW).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let checks = checks(&test);
    assert_eq!(checks.len(), 23, "{checks:?}");
    assert!(
        checks.iter().all(|check| check.starts_with("ok")),
        "{checks:?}"
    );
    assert!(checks[7].contains("1280x800 display"), "{checks:?}");
    assert!(checks[8].contains("DejaVuSansMono.ttf"), "{checks:?}");
    cleanup(&settings);
}

#[test]
fn the_calls_the_filter_refused_are_read_from_their_audit_records() {
    let settings = settings("runtime-console-denied");
    let test = Test::new(&settings, "runtime-rust-console").unwrap();
    let records = "zbmtest-call audit: type=1326 comm=\"zbm\" syscall=99 zbmtest-call-end\r\n\
        zbmtest-call audit: type=1326 comm=\"zbm\" syscall=315 zbmtest-call-end\r\n";
    let (mut vm, guest) = scripted(&test, vec![Step::Say(records)]);
    let monitor = Monitor { dump: no_dump, key };

    let calls = fallback::denied_calls(&mut vm, &monitor, NOW).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[0].ends_with("syscall=99"), "{calls:?}");
    assert!(calls[1].ends_with("syscall=315"), "{calls:?}");
    cleanup(&settings);
}

#[test]
fn a_console_that_shows_nothing_fails_its_checks() {
    let settings = settings("runtime-console-fail");
    let test = Test::new(&settings, "runtime-rust-console").unwrap();
    let mirror = mirror_path(&test, "virtio");
    fs::write(&mirror, "").unwrap();
    let (mut vm, guest) = scripted(&test, Vec::new());
    let monitor = Monitor { dump: no_dump, key };

    check_console(&test, &mut vm, &mirror, &monitor, NOW).unwrap();
    check_handover(&test, &mut vm, &mirror, NOW).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let checks = checks(&test);
    assert_eq!(checks.len(), 28, "{checks:?}");
    assert!(
        checks.iter().all(|check| check.starts_with("FAIL")),
        "{checks:?}"
    );
    cleanup(&settings);
}

#[test]
fn the_console_hands_the_display_back_and_the_environment_boots() {
    let settings = settings("runtime-console-handover");
    let test = Test::new(&settings, "runtime-rust-console").unwrap();
    let mirror = mirror_path(&test, "simpledrm");
    fs::write(
        &mirror,
        format!(
            "\x0czbm console: on /dev/dri/card0: 1024x768 display, ter-v14b.psf 8x14 cells, \
             128x54\n\
             \x0cBooting tplain/ROOT/be in 4 seconds\n\x0c{RELEASED}\n"
        ),
    )
    .unwrap();
    let (mut vm, guest) = scripted(&test, vec![Step::Say("BE-INIT-DONE\r\n")]);

    check_handover(&test, &mut vm, &mirror, NOW).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let checks = checks(&test);
    assert_eq!(checks.len(), 5, "{checks:?}");
    assert!(
        checks.iter().all(|check| check.starts_with("ok")),
        "{checks:?}"
    );
    cleanup(&settings);
}

#[test]
fn the_saved_kernel_log_is_noted_by_zbm_lines_alone() {
    let log = "[    0.0] Command line: zbm.console=drm\n\
               [    1.2] ZFSBootMenu: the console on the display ended (wait status 134)\n\
               [    1.3] zbm: console crashed: overflow; see crash-1.txt (at a.rs:1:2)\n\
               [    1.4] virtio_gpu: something\n";

    assert_eq!(
        zbm_lines(log),
        "[    1.2] ZFSBootMenu: the console on the display ended (wait status 134) | \
         [    1.3] zbm: console crashed: overflow; see crash-1.txt (at a.rs:1:2)"
    );
    assert_eq!(
        zbm_lines("[ 0.0] Command line: zbm.console=drm\n"),
        "(nothing)"
    );
    let many: String = (0..30)
        .map(|n| format!("ZFSBootMenu: line {n}\n"))
        .collect();
    assert!(zbm_lines(&many).starts_with("ZFSBootMenu: line 10 |"));
}

#[test]
fn the_console_s_confinement_is_typed_in_keys_the_test_has() {
    for line in [
        fallback::STATUS_LOG,
        fallback::COUNTS_LOG,
        fallback::DENIED_LOG,
    ] {
        assert!(key_names(line).is_some(), "{line}");
        assert!(line.ends_with("> /dev/ttyS0\n"), "{line}");
    }
}
