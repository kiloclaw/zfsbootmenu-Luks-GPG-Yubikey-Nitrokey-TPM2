//! The shell's completion of `zsnapshots`, called as Tab calls it.

use pretty_assertions::assert_eq;

use super::*;

/// Plays the shell's answer `said` to [`completion`]; the checks it
/// recorded.
fn completion_checks(name: &str, said: &'static str) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(
        &test,
        vec![
            Step::Hear(concat!(
                "COMP_CWORD=1; _zbm_complete zsnapshots tplain/; ",
                "echo \"<${COMPREPLY[*]}>\"; echo RC=$?"
            )),
            Step::Say(said),
        ],
    );

    completion(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

#[test]
fn tab_after_zsnapshots_offers_the_boot_environment_alone() {
    let found = completion_checks(
        "runtime-recovery-completion",
        "<tplain/ROOT/be>\r\nRC=0\r\n",
    );

    assert_eq!(
        found,
        ["ok   - Tab after zsnapshots tplain/ offers tplain/ROOT/be alone (0)"]
    );
}

#[test]
fn a_completion_that_offers_more_or_nothing_fails() {
    for (name, said) in [
        (
            "runtime-recovery-completion-more",
            "<tplain/ROOT/be tplain/ROOT>\r\nRC=0\r\n",
        ),
        ("runtime-recovery-completion-none", "<>\r\nRC=0\r\n"),
    ] {
        let found = completion_checks(name, said);

        assert_eq!(
            found,
            ["FAIL - Tab after zsnapshots tplain/ offers tplain/ROOT/be alone (0)"],
            "{name}"
        );
    }
}
