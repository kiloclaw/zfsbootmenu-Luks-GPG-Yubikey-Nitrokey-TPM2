use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

#[test]
fn the_shell_and_another_boot_ask_for_the_pin_of_the_locked_menu() {
    let settings = settings("integrity-locked-menu");
    let test = Test::new(&settings, "integrity-locked").unwrap();
    let script = vec![
        Step::Say("\x1b[2J\x1b[1;1H BOOT ENVIRONMENTS \r\n Enter Boot   e Edit PIN \r\n"),
        Step::Keys("!"),
        Step::Keys("\r"),
        Step::Say(
            "emergency shell\r\nThis ZFSBootMenu is locked. Type its PIN, or a blank line\r\n",
        ),
        Step::Say("PIN: "),
        Step::Hear("not the PIN"),
        Step::Say("\r\nWrong PIN.\r\nPIN: "),
        Step::Hear(PIN),
        Step::Pause(1100),
        Step::Say("zfsbootmenu ~ > "),
        Step::Hear("stty -echo cols 250; export PS1='zbm# ' PS2=''"),
        Step::Hear("true; echo RC=$?"),
        Step::Say("RC=0\r\nzbm# "),
        Step::Hear("echo ZBMTEST-LOCKED-$((6*7)); echo RC=$?"),
        Step::Say("ZBMTEST-LOCKED-42\r\nRC=0\r\nzbm# "),
        Step::Hear(SHELL_CHROOT),
        Step::Say("This ZFSBootMenu is locked. Type its PIN, or a blank line\r\n"),
        Step::Say("PIN: "),
        Step::Hear(PIN),
        Step::Say(
            "\x1b[H\x1b[2J[ESCAPE] Enter r/w chroot\r\n[RETURN] Enter r/o chroot\r\n\
             \r\nEntering chroot in 10 seconds",
        ),
        Step::Keys("\r"),
        Step::Say(
            "\x1b[H\x1b[2J* tplain/ROOT/be is mounted read-only\r\n\
             * /tmp is mounted read/write\r\n\r\ntplain/ROOT/be / > ",
        ),
        Step::Hear("cat /etc/os-release; echo RC=$?"),
        Step::Say("NAME=\"ZBM test boot environment\"\r\nRC=0\r\n"),
        Step::Hear("exit"),
        Step::Say("ZFS-CHROOT-RC=0\r\nzbm# "),
        Step::Hear("exit"),
        Step::Say("\x1b[2J\x1b[1;1H BOOT ENVIRONMENTS \r\n"),
        Step::Keys("\x1b[A"),
        Step::Keys("\r"),
        Step::Say("This ZFSBootMenu is locked. PIN (blank to go back): "),
        Step::Hear(PIN),
    ];
    let (mut vm, guest) = scripted(&test, script);

    marked(&test, &mut vm);
    shell_behind_the_pin(&test, &mut vm).unwrap();
    test.check(
        vm.expect1(LOCKED_ACTION, 0).is_err(),
        "nothing asked before the boot",
    );
    let _ = boot_behind_the_pin(&test, &mut vm);

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    assert_eq!(found.len(), 12, "{found:#?}");
    assert!(
        found.iter().all(|line| line.starts_with("ok")),
        "{found:#?}"
    );
}

#[test]
fn the_default_boot_of_a_locked_image_asks_nothing() {
    let settings = settings("integrity-locked-default");
    let test = Test::new(&settings, "integrity-locked").unwrap();

    check_default(&test, &Boot::default(), "Booting tauto/ROOT/be\n");
    check_default(
        &test,
        &Boot::default(),
        "This ZFSBootMenu is locked. PIN (blank",
    );

    let found = checks(&test);
    cleanup(&settings);
    assert!(
        found[0].starts_with("ok") && found[1].starts_with("FAIL"),
        "{found:#?}"
    );
    assert!(LOCK.contains("$argon2id$v=19$m=65536,t=3,p=1$"));
    assert!(!LOCK.contains(PIN));
}
