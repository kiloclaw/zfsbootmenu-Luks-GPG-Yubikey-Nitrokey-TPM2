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

#[test]
fn screens_are_read_from_the_mirror() {
    let settings = settings("runtime-console-mirror");
    let test = Test::new(&settings, "runtime-rust-console").unwrap();
    let path = mirror_path(&test, "virtio");
    fs::write(&path, "\x0cone\r\n\x0ctwo 42\r\n").unwrap();

    assert_eq!(
        wait_screen(&path, r"two \d+", 0).unwrap().as_deref(),
        Some("two 42\n")
    );
    assert_eq!(wait_screen(&path, "three", 0).unwrap(), None);
    assert_eq!(
        wait_screen(&test.dir().join("none"), "one", 0).unwrap(),
        None
    );
    assert!(wait_screen(&path, "(", 0).is_err());
    assert_eq!(last_screen(&path), "two 42");
    fs::write(&path, "\x0ca\r\n\r\nb\r\n\x0c\n").unwrap();
    assert_eq!(last_screen(&path), "a | b");
    assert_eq!(last_screen(&test.dir().join("none")), "(nothing)");
    cleanup(&settings);
}

#[test]
fn the_machine_has_the_display_and_the_mirror_s_serial_port() {
    let virtio = machine_extra("virtio", Path::new("/w/m.txt"));
    let simpledrm = machine_extra("simpledrm", Path::new("/w/m.txt"));

    assert_eq!(
        virtio,
        [
            "-vga",
            "none",
            "-device",
            "virtio-vga",
            "-chardev",
            "file,id=mirror,path=/w/m.txt",
            "-serial",
            "chardev:mirror"
        ]
    );
    assert_eq!(simpledrm, virtio[4..]);
    let spec = spec();
    assert_eq!(spec.runtime, Some(ZbmRuntime::Rust));
    assert!(spec.conf.contains("virtio_gpu evdev"), "{}", spec.conf);
    // The mirror the test reads is in test builds only.
    assert_eq!(spec.features.as_deref(), Some("zbm/console-mirror"));
}

const LOGGED: &str = "speed 38400 baud; isig icanon\r\nSigIgn: 0000000000000000\r\n\
    zbm-state-end\r\nzbm-flood-ok\r\nzbm-back-/dev/pts/1\r\n\
    [   40.2] ZFSBootMenu: the console on the display ended (wait status 9); \
    going on on the kernel console\r\n\
    [   41.2] ZFSBootMenu: the console on the display started again (process 812)\r\n\
    [    3.1] ZFSBootMenu: console: the menu draws on its cells (client channel version 1)\r\n\
    CapEff:\t0000000004004000\r\nNoNewPrivs:\t1\r\nSeccomp:\t2\r\nSeccomp_filters:\t1\r\n\
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
         \x0cBooting tplain/ROOT/be in 29 seconds\n\x0c BOOT ENVIRONMENTS \n\
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
    assert_eq!(checks.len(), 21, "{checks:?}");
    assert!(
        checks.iter().all(|check| check.starts_with("ok")),
        "{checks:?}"
    );
    assert!(checks[6].contains("1280x800 display"), "{checks:?}");
    assert!(checks[7].contains("DejaVuSansMono.ttf"), "{checks:?}");
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
    assert_eq!(checks.len(), 26, "{checks:?}");
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
    for line in [fallback::STATUS_LOG, fallback::COUNTS_LOG] {
        assert!(key_names(line).is_some(), "{line}");
        assert!(line.ends_with("> /dev/ttyS0\n"), "{line}");
    }
}
