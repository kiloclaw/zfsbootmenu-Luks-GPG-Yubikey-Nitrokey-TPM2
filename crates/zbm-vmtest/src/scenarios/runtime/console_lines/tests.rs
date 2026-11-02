use std::fs;

use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// What the line view prints of the two pools' boot environments.
const PAGE: &str = "\r\nBoot Environments\r\n  1) tplain/ROOT/be, default, 1 kernel\r\n  \
    2) tforeign/ROOT/be, 1 kernel\r\n\r\n\
    Type a number to choose it, or a number and a key, as 2 k.\r\n";

/// The checks of `test`, after its group's directory is gone.
fn found_checks(test: &Test, settings: &crate::settings::Settings) -> Vec<String> {
    let found = checks(test);
    cleanup(settings);
    found
}

#[test]
fn the_number_of_tforeign_s_environment_is_typed_after_the_list() {
    let settings = settings("runtime-console-lines-choose");
    let test = Test::new(&settings, "runtime-rust-console-lines").unwrap();
    let script = vec![Step::Say(PAGE), Step::Keys("2\r")];
    let (mut vm, guest) = scripted(&test, script);

    let chosen = choose_foreign(&mut vm, "dumb", Duration::ZERO).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = found_checks(&test, &settings);
    assert!(chosen);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(
        found.iter().all(|line| line.starts_with("ok")),
        "{found:#?}"
    );
}

#[test]
fn a_screen_without_the_list_fails_and_types_nothing() {
    let settings = settings("runtime-console-lines-none");
    let test = Test::new(&settings, "runtime-rust-console-lines").unwrap();
    let script = vec![Step::Say("BOOT ENVIRONMENTS\r\n> tforeign/ROOT/be\r\n")];
    let (mut vm, guest) = scripted(&test, script);

    let chosen = choose_foreign(&mut vm, "dumb", Duration::ZERO).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = found_checks(&test, &settings);
    assert!(!chosen);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].starts_with("FAIL"), "{found:#?}");
}

#[test]
fn zfsbootmenu_s_boot_line_after_its_teardown_says_which_environment_booted() {
    let settings = settings("runtime-console-lines-booted");
    let test = Test::new(&settings, "runtime-rust-console-lines").unwrap();
    // The boot line comes after the teardown report, where the boot was
    // followed to.
    let script = vec![Step::Say(
        "ZBMTEST-TEARDOWN dm=[] pools=[tforeign tplain ] state=[]\r\n\r\n\
         Booting /boot/vmlinuz-7.3.0-9-generic for tforeign/ROOT/be ...\r\n",
    )];
    let (mut vm, guest) = scripted(&test, script);

    let found = booted(&mut vm);
    check_booted(&test, "dumb", &found);
    check_booted(&test, "lines", "tplain/ROOT/be");
    check_booted(&test, "lines", "");

    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(found, "tforeign/ROOT/be");
    let found = found_checks(&test, &settings);
    let passed: Vec<bool> = found.iter().map(|line| line.starts_with("ok")).collect();
    assert_eq!(passed, [true, false, false], "{found:#?}");
}

#[test]
fn the_shell_reads_its_own_output_back_from_the_console_s_text() {
    let settings = settings("runtime-console-lines-vcsa");
    let test = Test::new(&settings, "runtime-rust-console-lines").unwrap();
    let mirror = test.dir().join("mirror-vcsa.txt");
    fs::write(&mirror, "BOOT ENVIRONMENTS\n\x0cemergency shell\n").unwrap();
    let script = vec![
        Step::Say("\x1b[H\x1b[2JBooting tplain/ROOT/be in 59 seconds\r\n"),
        Step::Keys("\x1b"),
        Step::Say("\x1b[H\x1b[2JBOOT ENVIRONMENTS\r\n"),
        Step::Keys("!"),
        Step::Keys("\r"),
        Step::Keys(VCS_COMMAND),
        Step::Say("zbm-42-vcs\r\nZBMTEST-VCS-2-yes\r\n"),
    ];
    let (mut vm, guest) = scripted(&test, script);

    check_vcsa(&mut vm, &mirror, Duration::ZERO).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = found_checks(&test, &settings);
    assert_eq!(found.len(), 4, "{found:#?}");
    assert!(
        found.iter().all(|line| line.starts_with("ok")),
        "{found:#?}"
    );
}

#[test]
fn the_shell_s_command_spells_its_marker_only_in_its_output() {
    assert!(!VCS_COMMAND.contains("zbm-42-vcs; "));
    assert!(VCS_COMMAND.contains("echo zbm-$((6*7))-vcs"));
    assert!(!VCS_COMMAND.contains("ZBMTEST-VCS-2-yes"));
    let lines: Vec<&str> = LINE_BOOTS.iter().map(|(_, extra)| *extra).collect();
    assert_eq!(lines, [" TERM=dumb", " zbm.menu=lines"]);
    assert!(DRM_APPEND.contains("zbm.console=drm"));
    assert!(!DRM_APPEND.contains("zbm.menu"));
}
