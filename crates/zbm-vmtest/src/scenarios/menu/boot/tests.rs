use pretty_assertions::assert_eq;

use super::*;
use crate::scenarios::menu::tests::{all_ok, checked, played, serial, started};
use crate::testing::Step as Play;

/// What a boot prints from ZFSBootMenu's reports to the boot environment's
/// end, which says `cmdline` and `root`.
fn booted(cmdline: &str, root: &str) -> Play {
    Play::Say(
        format!(
            "ZBMTEST-STATUS-BEGIN\r\nZS|   pool: tmenu\r\nZS|  state: ONLINE\r\n\
             ZBMTEST-STATUS-END\r\nZBMTEST-TEARDOWN dm=[] pools=[ tmenu ] state=[]\r\n\
             BE-INIT-START\r\nBE-CMDLINE: {cmdline}\r\nBE-KERNEL: 6.8.0-zbm\r\n\
             BE-ROOT: {root} zfs rw\r\nBE-INIT-DONE\r\n"
        )
        .leak(),
    )
}

#[test]
fn the_first_boot_tours_the_palette_then_boots_the_kernel_edited() {
    let mut script = started(true);
    script.extend(played(steps::BOOT_TOUR));
    script.extend(played(steps::BOOT_KERNEL));
    script.push(booted(
        "root=zfs:tmenu/ROOT/be console=ttyS0 loglevel=4 zbmtest.edited=1",
        "tmenu/ROOT/be",
    ));

    let found = checked("boot-first", script, |test, vm| {
        let boot = first(test, vm, &mut serial())?;
        check_edited(test, &boot);
        Ok(())
    });

    assert!(all_ok(&found), "{found:#?}");
    assert!(found.len() > 10, "{found:#?}");
}

#[test]
fn the_second_boot_boots_a_clone_of_five() {
    for (root, passed) in [("tmenu/ROOT/booted", true), ("tmenu/ROOT/be", false)] {
        let mut script = started(false);
        script.extend(played(steps::BOOT_CLONE));
        script.push(booted("root=zfs:tmenu/ROOT/booted", root));

        let found = checked(&format!("boot-second-{passed}"), script, |test, vm| {
            let boot = second(test, vm, &mut serial())?;
            check_clone(test, &boot);
            Ok(())
        });

        let last = found.last().unwrap();
        assert_eq!(last.starts_with("ok"), passed, "{found:#?}");
    }
}

#[test]
fn the_record_names_the_kernel_and_the_command_line_edited() {
    let good = concat!(
        r#"{"schema":1,"kernel":"/boot/vmlinuz-0.0.1-zbmtest","#,
        r#""command_line":"root=zfs:tmenu/ROOT/be zbmtest.edited=1","#,
        r#""command_line_edited":true}"#
    );
    let other = concat!(
        r#"{"kernel":"/boot/vmlinuz-6.8.0","command_line":"root=zfs:tmenu/ROOT/be","#,
        r#""command_line_edited":false}"#
    );

    let found = checked("boot-record", Vec::new(), |test, _| {
        check_record(test, good);
        check_record(test, other);
        check_record(test, "");
        check_edited(test, &Boot::default());
        Ok(())
    });

    let passed: Vec<bool> = found.iter().map(|line| line.starts_with("ok")).collect();
    assert_eq!(passed, [true, true, false, false, false, false, false]);
}
