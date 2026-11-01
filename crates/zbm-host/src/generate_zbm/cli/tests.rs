//! Tests of the command line.

use pretty_assertions::assert_eq;

use super::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

fn run(list: &[&str]) -> Options {
    match parse(&args(list)).outcome {
        Outcome::Run(options) => *options,
        other => panic!("{list:?} gave {other:?}"),
    }
}

#[test]
fn the_documented_options_set_their_fields() {
    let options = run(&[
        "-v",
        "3.1",
        "-k",
        "/boot/k",
        "-K",
        "6.1",
        "-p",
        "vmlinuz",
        "-b",
        "/b",
        "-C",
        "/c",
        "-l",
        "quiet",
        "-c",
        "-d",
        "--enable",
        "--disable",
        "--no-initcpio",
        "-H",
        "a",
        "--hookd=b",
        "-V",
        "operand",
    ]);
    assert_eq!(options.version.as_deref(), Some("3.1"));
    assert_eq!(options.kernel.as_deref(), Some("/boot/k"));
    assert_eq!(options.kernel_version.as_deref(), Some("6.1"));
    assert_eq!(options.kernel_prefix.as_deref(), Some("vmlinuz"));
    assert_eq!(options.bootdir, "/b");
    assert_eq!(options.confd.as_deref(), Some("/c"));
    assert_eq!(options.cmdline.as_deref(), Some("quiet"));
    // A value is the next argument, whatever it looks like
    assert_eq!(options.config, "-d");
    assert!(!options.debug && options.enable && options.disable && options.showver);
    assert_eq!(options.initcpio, Some(false));
    assert_eq!(options.hookd, Some(vec!["a".to_owned(), "b".to_owned()]));
    assert_eq!(run(&["-i"]).initcpio, Some(true));
    assert!(run(&["--debug"]).debug);
    assert!(!run(&["--", "-d"]).debug);
    assert_eq!(run(&[]), Options::default());
}

#[test]
fn other_spellings_work_with_a_warning_for_one_release() {
    let parsed = parse(&args(&["--kv", "6.1", "-config=foo", "--init", "+debug"]));
    let Outcome::Run(options) = parsed.outcome else {
        panic!("{parsed:?}");
    };
    assert_eq!(options.kernel_version.as_deref(), Some("6.1"));
    assert_eq!(options.config, "foo");
    assert_eq!(options.initcpio, Some(true));
    assert!(options.debug);
    assert_eq!(parsed.warnings.len(), 4);
    assert!(parsed.warnings[0].starts_with("generate-zbm: --kv is not how the manual spells"));
    assert!(
        parsed
            .warnings
            .iter()
            .all(|warning| warning.ends_with("next release\n"))
    );
}

#[test]
fn errors_and_help() {
    for (list, warning) in [
        (&["--bogus"][..], "Unknown option: bogus\n"),
        (&["-x"], "Unknown option: x\n"),
        (&["--debug=1"], "Option debug does not take an argument\n"),
        (&["--kernel"], "Option kernel requires an argument\n"),
        (&["--k"], "Option k is ambiguous (--kernel, --kver)\n"),
    ] {
        let parsed = parse(&args(list));
        assert_eq!(parsed.outcome, Outcome::Invalid, "{list:?}");
        assert_eq!(parsed.warnings, [warning], "{list:?}");
    }
    assert_eq!(
        parse(&args(&["-d", "-h", "--bogus"])).outcome,
        Outcome::Help
    );
    assert_eq!(parse(&args(&["--help"])).outcome, Outcome::Help);
}
