//! The shell's `zsnapshots`, which the session serves as a menu.

use pretty_assertions::assert_eq;

use super::*;

/// Plays the guest's `menu`, after `zsnapshots`, then `said` once the keys
/// of the recovery shell came, against [`zsnapshots`]; the checks it
/// recorded.
fn snapshots_checks(name: &str, menu: &'static str, said: &'static str) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(
        &test,
        vec![
            Step::Hear("clear; zsnapshots tplain/ROOT/be; echo ZSNAPSHOTS-RC=$?"),
            Step::Say(menu),
            Step::Keys("!"),
            Step::Keys("\r"),
            Step::Quiet(500),
            Step::Say(said),
        ],
    );

    zsnapshots(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

/// The menu on the Snapshots tab of `tplain`'s environment.
const SNAPSHOTS_TAB: &str = "\x1b[H\x1b[2J  1 Boot [2 Snapshots] 3 Pools  4 Log\r\n \
    tplain > tplain/ROOT/be\r\n+ ENVIRONMENTS -- 1 ++ SNAPSHOTS BY NAME -- 0 +\r\n";

#[test]
fn the_shell_s_zsnapshots_opens_the_snapshots_tab_and_its_recovery_shell_ends_it() {
    let found = snapshots_checks(
        "runtime-recovery-zsnapshots",
        SNAPSHOTS_TAB,
        "\x1b[H\x1b[2JZSNAPSHOTS-RC=0\r\nzbm# ",
    );

    assert_eq!(
        found,
        [
            "ok   - zsnapshots opens the menu on the Snapshots tab of tplain/ROOT/be",
            "ok   - ! and Enter leave the menu, back at the prompt, zsnapshots ending with 0",
        ]
    );
}

#[test]
fn a_zsnapshots_that_shows_no_snapshots_tab_fails() {
    let settings = settings("runtime-recovery-zsnapshots-refused");
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    // The menu refused the name: the shell's prompt is back at once, and
    // the guest is gone before the keys that would leave a menu.
    let (mut vm, guest) = scripted(
        &test,
        vec![
            Step::Hear("clear; zsnapshots tplain/ROOT/be; echo ZSNAPSHOTS-RC=$?"),
            Step::Say(
                "zsnapshots: tplain/ROOT/be is not a boot environment\r\nZSNAPSHOTS-RC=1\r\nzbm# ",
            ),
        ],
    );

    let done = zsnapshots(&test, &mut vm);

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    assert!(done.is_err());
    assert_eq!(
        found,
        ["FAIL - zsnapshots opens the menu on the Snapshots tab of tplain/ROOT/be"]
    );
}
