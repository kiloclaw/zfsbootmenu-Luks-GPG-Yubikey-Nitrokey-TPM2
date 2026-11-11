//! The shell's `zreport` through a pipe.

use pretty_assertions::assert_eq;

use super::*;

/// Plays the shell's answer `said` to [`zreport`]; the checks it recorded.
fn report_checks(name: &str, said: &'static str) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(
        &test,
        vec![
            Step::Hear("{ zreport; echo ZREPORT-STATUS=$?; } | cat; echo RC=$?"),
            Step::Say(said),
        ],
    );

    zreport(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

#[test]
fn the_shell_s_zreport_lists_the_luks_close_among_the_built_in_steps() {
    let found = report_checks(
        "runtime-recovery-zreport",
        "\x1b[1;37mSystem Report\r\n\r\n\x1b[0m\x1b[0;33m\r\n>> Built-in steps\r\n\x1b[0m\
         * \x1b[0;32mteardown.d/80-luks-close.sh\x1b[0m\r\n\
         * \x1b[0;32mteardown.d/90-xhci-unbind.sh\x1b[0m\r\nZREPORT-STATUS=0\r\nRC=0\r\n",
    );

    assert_eq!(
        found,
        [
            "ok   - zreport exits 0 through a pipe",
            "ok   - zreport lists the built-in step teardown.d/80-luks-close.sh, which runs",
        ]
    );
}

#[test]
fn a_legacy_zreport_or_a_skipped_step_fails() {
    // The legacy function lists the hooks' files only.
    let legacy = report_checks(
        "runtime-recovery-zreport-legacy",
        "\x1b[0;33m\r\n>> Enabled hooks\r\n\x1b[0m\
         * /libexec/hooks/teardown.d/80-luks-close.sh\r\nZREPORT-STATUS=0\r\nRC=0\r\n",
    );
    let skipped = report_checks(
        "runtime-recovery-zreport-skipped",
        ">> Built-in steps\r\n\
         * teardown.d/80-luks-close.sh (skipped by zbm.skip_hooks)\r\nZREPORT-STATUS=1\r\nRC=0\r\n",
    );

    let failed = "FAIL - zreport lists the built-in step teardown.d/80-luks-close.sh, which runs";
    assert_eq!(legacy[1], failed);
    assert_eq!(skipped, ["FAIL - zreport exits 0 through a pipe", failed]);
}
