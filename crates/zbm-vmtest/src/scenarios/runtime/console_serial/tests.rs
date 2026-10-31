use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// Presses nothing: the screens are in the mirror already.
fn no_key(_vm: &Vm<'_>, _key: &str) -> Result<()> {
    Ok(())
}

/// The pace of the scripted VM.
const QUICK: Pace = Pace {
    pause: Duration::ZERO,
    patience: 30,
};

#[test]
fn the_checks_pass_on_the_screens_they_expect() {
    let settings = settings("runtime-console-serial");
    let test = Test::new(&settings, "runtime-rust-console-serial").unwrap();
    let mirror = test.dir().join("mirror-serial.txt");
    let screens = "Booting tplain/ROOT/be in 59 seconds\n\x0c\
                   BOOT ENVIRONMENTS\n\x0c\
                   emergency shell\n\x0c\
                   zbm# echo zbm-serial-$(stty size)\nzbm-serial-25 80\n\x0c\
                   zbm-display-50 160\n\x0c\
                   zbm-again-25 80\n";
    fs::write(&mirror, screens).unwrap();
    let script = vec![
        Step::Say("\x1b[H\x1b[2JBooting tplain/ROOT/be in 59 seconds\r\n"),
        Step::Keys("\x1b"),
        Step::Say("\x1b[H\x1b[2JBOOT ENVIRONMENTS\r\n"),
        Step::Keys("!"),
        Step::Keys("\r"),
        Step::Keys("echo zbm-serial-$(stty size)\r"),
        Step::Keys("\x0c"),
        Step::Pause(300),
        Step::Say("\x1b[?25l\x1b[0m\x1b[H\x1b[2Jzbm# \x1b[?25h"),
        Step::Keys("echo zbm-again-$(stty size)\r"),
    ];
    let (mut vm, guest) = scripted(&test, script);
    let serial = Serial {
        test: &test,
        mirror: &mirror,
        key: no_key,
        pace: QUICK,
    };

    serial.check(&mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    assert_eq!(found.len(), 10, "{found:#?}");
    assert!(
        found.iter().all(|line| line.starts_with("ok")),
        "{found:#?}"
    );
}

#[test]
fn a_serial_command_that_sees_the_display_s_size_fails() {
    let settings = settings("runtime-console-serial-size");
    let test = Test::new(&settings, "runtime-rust-console-serial").unwrap();
    let mirror = test.dir().join("mirror-serial.txt");
    let wide = format!("zbm-serial-50 160\n{}\n", "x".repeat(160));
    fs::write(&mirror, wide).unwrap();
    let script = vec![Step::Keys("echo zbm-serial-$(stty size)\r")];
    let (mut vm, guest) = scripted(&test, script);
    let serial = Serial {
        test: &test,
        mirror: &mirror,
        key: no_key,
        pace: QUICK,
    };

    serial.check_serial_size(&mut vm, "serial").unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(
        found.iter().all(|line| line.starts_with("FAIL")),
        "{found:#?}"
    );
}

#[test]
fn sizes_widths_and_clears_are_read() {
    assert_eq!(size_in("$\nzbm-x-25 80  \n", "x"), Some((80, 25)));
    assert_eq!(size_in("zbm-x-25 80\n", "y"), None);
    assert_eq!(size_in("echo zbm-x-$(stty size)\n", "x"), None);
    assert_eq!(widest("ab   \nabcd\n\n"), 4);
    assert_eq!(widest(""), 0);
    let settings = settings("runtime-console-serial-clears");
    let test = Test::new(&settings, "runtime-rust-console-serial").unwrap();
    let log = test.dir().join("serial.log");
    fs::write(&log, b"\x1b[2J text \x1b[2J\x1b[2").unwrap();
    let counted = clears(&log);
    let missing = clears(&test.dir().join("missing.log"));
    cleanup(&settings);
    assert_eq!((counted, missing), (2, 0));
}

#[test]
fn the_commands_have_keys_and_the_serial_console_comes_last() {
    assert!(key_names("echo zbm-display-$(stty size)\n").is_some());
    let consoles: Vec<&str> = APPEND
        .split_whitespace()
        .filter(|word| word.starts_with("console="))
        .collect();
    assert_eq!(consoles, ["console=tty1", "console=ttyS0"]);
    assert!(!APPEND.contains("zbm.console_serial"));
}
