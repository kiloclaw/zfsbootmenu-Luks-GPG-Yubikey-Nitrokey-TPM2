//! The shell's `command -v fzf`, which fails in a Rust image.

use pretty_assertions::assert_eq;

use super::*;

/// Plays the shell's answer `said` to [`no_fzf`]; the checks it recorded.
fn fzf_checks(name: &str, said: &'static str) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(
        &test,
        vec![Step::Hear("command -v fzf; echo RC=$?"), Step::Say(said)],
    );

    no_fzf(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

#[test]
fn the_shell_finds_no_fzf_in_a_rust_image() {
    let found = fzf_checks("runtime-recovery-no-fzf", "RC=1\r\n");

    assert_eq!(
        found,
        ["ok   - the image has no fzf: command -v fzf fails (1)"]
    );
}

#[test]
fn an_image_with_fzf_fails() {
    let found = fzf_checks("runtime-recovery-fzf", "/usr/bin/fzf\r\nRC=0\r\n");

    assert_eq!(
        found,
        ["FAIL - the image has no fzf: command -v fzf fails (0)"]
    );
}
