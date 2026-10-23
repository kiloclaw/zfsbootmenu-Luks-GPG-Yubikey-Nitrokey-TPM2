use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

const REPORT: &str = "ZFSBootMenu crash report\n\
                      reason: panicked at crates/zbm/src/debug.rs:42:5: zbm debug panic\n\
                      span: crash_test{reason=zbm debug panic}\n";

#[test]
fn the_crash_report_and_the_counts_are_read_from_the_console() {
    let settings = settings("runtime-crash-read");
    let test = Test::new(&settings, "runtime-rust-crash").unwrap();
    let script = vec![Step::Say(
        "ZBMTEST-CRASH-BEGIN\r\n\r\nZFSBootMenu crash report\r\n\
         reason: panicked at crates/zbm/src/debug.rs:42:5: zbm debug panic\r\n\
         span: crash_test{reason=zbm debug panic}\r\n\r\nZBMTEST-CRASH-END\r\n\
         ZBMTEST-CRASH-KMSG 1\r\nZBMTEST-CRASH-JOURNAL 2\r\n",
    )];
    let (mut vm, guest) = scripted(&test, script);

    let crash = read_crash(&mut vm).unwrap();

    guest.finish();
    assert_eq!(
        crash,
        Crash {
            report: REPORT.to_owned(),
            kmsg: 1,
            journal: 2
        }
    );
    cleanup(&settings);
}

#[test]
fn the_report_must_name_the_panic_and_its_span() {
    let settings = settings("runtime-crash-checks");
    let test = Test::new(&settings, "runtime-rust-crash").unwrap();
    let good = Crash {
        report: REPORT.to_owned(),
        kmsg: 1,
        journal: 1,
    };

    check_crash(&test, &good);
    check_crash(&test, &Crash::default());

    assert_eq!(
        checks(&test),
        [
            "ok   - zbm debug crash printed the report",
            "ok   - the report says where the panic was",
            "ok   - the report names the open span",
            "ok   - the kernel log names the crash",
            "ok   - the journal holds the report",
            "FAIL - zbm debug crash printed the report",
            "FAIL - the report says where the panic was",
            "FAIL - the report names the open span",
            "FAIL - the kernel log names the crash",
            "FAIL - the journal holds the report",
        ]
    );
    cleanup(&settings);
}

#[test]
fn the_symbolized_report_must_name_the_function_and_line() {
    let settings = settings("runtime-crash-symbolized");
    let test = Test::new(&settings, "runtime-rust-crash").unwrap();
    let good = Ran {
        code: 0,
        stdout: "  #3 zbm::debug::panic_for_test at /src/crates/zbm/src/debug.rs:42\n".into(),
        stderr: String::new(),
    };
    let missing = Ran {
        code: 127,
        ..Ran::default()
    };

    check_symbolized(&test, &good, &[]).unwrap();
    check_symbolized(&test, &missing, &["cargo", "addr2line"]).unwrap();

    assert_eq!(
        checks(&test),
        [
            "ok   - cargo xtask symbolize ran",
            "ok   - the symbolized report names the function and its line",
            "FAIL - cargo xtask symbolize ran (missing: cargo, addr2line)",
            "FAIL - the symbolized report names the function and its line",
        ]
    );
    cleanup(&settings);
}

#[test]
fn host_commands_report_their_status_and_output() {
    let args: Vec<String> = ["sh", "-c", "pwd; echo err >&2; exit 3"]
        .iter()
        .map(|&arg| arg.to_owned())
        .collect();

    let ran = run_host(&args, Path::new("/")).unwrap();

    assert_eq!(
        ran,
        Ran {
            code: 3,
            stdout: "/\n".into(),
            stderr: "err\n".into()
        }
    );
    assert!(run_host(&[], Path::new("/")).is_err());
}
