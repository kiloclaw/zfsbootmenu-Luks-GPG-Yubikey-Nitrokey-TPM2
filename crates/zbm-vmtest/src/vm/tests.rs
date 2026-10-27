//! Tests of the shell output the machine gives.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn shell_output_drops_prompts_and_blank_lines() {
    let before = "zbm# zpool list -H -o name\ntank\n\n  \nzbm#\nzbm# mixed";

    assert_eq!(shell_output(before), "zpool list -H -o name\ntank\nmixed");
}

#[test]
fn the_echoed_command_is_not_output() {
    let sent = [
        "for i in 1 2".to_owned(),
        "do echo $i; done; echo RC=$?".to_owned(),
    ];

    assert_eq!(
        without_echo("for i in 1 2\ndo echo $i; done; echo RC=$?\n1\n2", &sent),
        "1\n2"
    );
    assert_eq!(without_echo("1\n2", &sent), "1\n2");
    assert_eq!(
        without_echo("for i in 1 2 \nfor i in 1 2", &sent),
        "for i in 1 2"
    );
    assert_eq!(
        without_echo("do echo $i; done; echo RC=$?", &sent),
        "do echo $i; done; echo RC=$?"
    );
}

#[test]
fn shell_options_combine() {
    let options = Sh::default().show().quiet().timeout(3600);

    assert_eq!(
        (options.timeout, options.show, options.quiet, options.pw),
        (3600, true, true, PW)
    );
}
