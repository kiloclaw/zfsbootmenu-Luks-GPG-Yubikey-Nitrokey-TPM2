use super::*;
use crate::scenarios::menu::tests::{
    all_ok, checked, played, pools_left_said, powered_off, serial, shell_left, shell_opened,
    started,
};

#[test]
fn the_pool_tasks_run_and_the_shell_checks_what_they_did() {
    let mut script = started(false);
    script.extend(played(steps::POOLS));
    script.extend(shell_opened());
    script.extend(pools_left_said());
    script.extend(shell_left());
    script.extend(powered_off());

    let found = checked("pools", script, |test, vm| drive(test, vm, &mut serial()));

    assert!(all_ok(&found), "{found:#?}");
    assert!(found.len() > 30, "{found:#?}");
}
