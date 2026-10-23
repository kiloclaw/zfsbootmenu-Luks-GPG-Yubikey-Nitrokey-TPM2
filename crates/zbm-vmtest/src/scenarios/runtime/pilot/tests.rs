use std::fs;

use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

#[test]
fn the_menu_pilot_lists_and_chooses_the_boot_environment() {
    let settings = settings("runtime-pilot-menu");
    let test = Test::new(&settings, "runtime-rust-pilot").unwrap();
    let script = vec![
        Step::Hear("TERM=vt100 /libexec/zbm zbm-menu-pilot --stats --size 80x24"),
        Step::Say(
            "2 environments\r\n\x1b[1;1H+Boot environments-----+\r\n\
             | \x1b[7mtplain/ROOT/be\x1b[0m |\r\n[Enter] boot [Ctrl-U] clear\r\n\
             chosen: tplain/ROOT/be\r\n12 frames, mean 1.50 ms, slowest 4 ms\r\n",
        ),
        Step::Hear("be\rtrue; echo RC=$?"),
        Step::Say("RC=0\r\n"),
    ];
    let (mut vm, guest) = scripted(&test, script);

    menu(&mut vm, "vt100", Some("80x24")).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(
        checks(&test),
        [
            "ok   - TERM=vt100: the boot environment is listed",
            "ok   - TERM=vt100: ASCII borders",
            "ok   - TERM=vt100: Enter chooses tplain/ROOT/be (tplain/ROOT/be)",
        ]
    );
    let steps = fs::read_to_string(test.dir().join("steps.log")).unwrap();
    assert!(steps.contains(
        "note - TERM=vt100: 12 frames, mean 1.5 ms, slowest 4.0 ms; \
         two filter keys took 0 serial bytes; Enter to answer "
    ));
    cleanup(&settings);
}

#[test]
fn a_menu_pilot_that_does_not_start_fails_with_its_message() {
    let settings = settings("runtime-pilot-fails");
    let test = Test::new(&settings, "runtime-rust-pilot").unwrap();
    let script = vec![
        Step::Hear("TERM=linux /libexec/zbm zbm-menu-pilot --stats"),
        Step::Say("zbm-menu-pilot: no terminal\r\n"),
    ];
    let (mut vm, guest) = scripted(&test, script);

    menu(&mut vm, "linux", None).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(
        checks(&test),
        ["FAIL - TERM=linux: the menu starts (zbm-menu-pilot: no terminal)"]
    );
    cleanup(&settings);
}

#[test]
fn statistics_are_written_as_python_writes_them() {
    assert_eq!(
        stats("12", "1.50", "4").unwrap(),
        (12, "1.5".to_owned(), "4.0".to_owned())
    );
    assert_eq!(py_float(0.25), "0.25");
    assert_eq!(py_float(16.0), "16.0");
    assert!(stats("1", "1.2.3", "4").is_err());
}

#[test]
fn the_unlock_pilot_commands_are_those_of_the_python_harness() {
    assert_eq!(
        format_command(),
        "if command -v truncate >/dev/null; then truncate -s 32M /tmp/pilot.img; \
         elif command -v fallocate >/dev/null; then fallocate -l 32M /tmp/pilot.img; \
         else head -c 33554432 /dev/zero > /tmp/pilot.img; fi && \
         dev=$(losetup -f --show /tmp/pilot.img) && printf %s zbm-pilot-key-1234 | \
         cryptsetup luksFormat --batch-mode --type luks2 --pbkdf pbkdf2 \
         --pbkdf-force-iterations 1000 --key-file=- \"$dev\" && \
         printf %s zbm-pilot-key-1234 > /tmp/pilot.key && printf wrong > /tmp/wrong.key && \
         echo DEV=$dev"
    );
    assert_eq!(
        open_command("wrong"),
        "zluks --pilot open \"$dev\" zbm-pilot --key-file /tmp/wrong.key \
         --journal /tmp/pilot.jsonl"
    );
}

#[test]
fn the_journal_must_record_both_attempts_without_the_key() {
    let settings = settings("runtime-pilot-journal");
    let test = Test::new(&settings, "runtime-rust-pilot").unwrap();

    check_journal(
        &test,
        r#"{"message":"refused","stdin":"secret"}
{"message":"opened","stdin":"secret"}"#,
    );
    check_journal(&test, r#"{"message":"opened","key":"zbm-pilot-key-1234"}"#);

    assert_eq!(
        checks(&test),
        [
            "ok   - the journal records that the key went to standard input",
            "ok   - the journal does not contain the key",
            "ok   - the journal records the refused and the successful attempt",
            "FAIL - the journal records that the key went to standard input",
            "FAIL - the journal does not contain the key",
            "FAIL - the journal records the refused and the successful attempt",
        ]
    );
    cleanup(&settings);
}
