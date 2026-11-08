use super::*;
use crate::scenarios::menu::tests::{
    LEFT, all_ok, checked, played, powered_off, serial, shell_left, shell_opened,
    snapshots_left_said, space_after_said, space_before_said, started,
};

#[test]
fn the_snapshot_tasks_run_and_the_shell_checks_what_they_did() {
    let mut script = started(true);
    script.extend(played(steps::SNAPSHOTS_CHANGES));
    script.extend(shell_opened());
    script.extend(space_before_said(8192, 12288));
    script.extend(shell_left());
    script.extend(played(steps::SNAPSHOTS_DESTROY));
    script.extend(shell_opened());
    script.extend(space_after_said(4096));
    script.extend(shell_left());
    script.extend(played(steps::SNAPSHOTS_REFUSED));
    script.extend(shell_opened());
    script.extend(snapshots_left_said(LEFT));
    script.extend(shell_left());
    script.extend(powered_off());

    let found = checked("snapshots", script, |test, vm| {
        drive(test, vm, &mut serial())
    });

    assert!(all_ok(&found), "{found:#?}");
    assert!(found.len() > 40, "{found:#?}");
}
