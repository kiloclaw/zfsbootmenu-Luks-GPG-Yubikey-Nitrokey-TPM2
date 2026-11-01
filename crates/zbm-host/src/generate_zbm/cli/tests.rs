//! Tests of the command line parser.

use super::*;
use zbm_testkit::parity::Mode;
use zbm_testkit::{Rng, parity};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

fn run(list: &[&str]) -> Options {
    match parse(&args(list), false).outcome {
        Outcome::Run(options) => *options,
        other => panic!("{list:?} gave {other:?}"),
    }
}

#[test]
fn names_and_values() {
    let options = run(&[
        "-c",
        "-d",
        "--kv",
        "6.1",
        "+V",
        "--no",
        "-H",
        "a",
        "--hookd=b",
        "x",
    ]);
    assert_eq!(options.config, "-d");
    assert_eq!(options.kernel_version.as_deref(), Some("6.1"));
    assert!(options.showver && !options.debug);
    assert_eq!(options.initcpio, Some(false));
    assert_eq!(options.hookd, Some(vec!["a".to_owned(), "b".to_owned()]));
    assert_eq!(run(&["-config=foo"]).config, "foo");
    assert_eq!(run(&["--init"]).initcpio, Some(true));
    assert!(!run(&["--", "-d"]).debug);
}

#[test]
fn errors() {
    let parsed = parse(
        &args(&["--con", "x", "-dV", "--config=", "--en=1", "+"]),
        false,
    );
    assert_eq!(parsed.outcome, Outcome::Invalid);
    assert_eq!(
        parsed.warnings,
        [
            "Option con is ambiguous (confd, config)\n",
            "Unknown option: dV\n",
            "Option config requires an argument\n",
            "Option enable does not take an argument\n",
            "Missing option after +\n",
        ]
    );
    let parsed = parse(&args(&["--x", "--help", "--y"]), false);
    assert_eq!(parsed.outcome, Outcome::Help);
    assert_eq!(parsed.warnings, ["Unknown option: x\n"]);
    let parsed = parse(&args(&["foo", "-d"]), true);
    assert_eq!(parsed.outcome, Outcome::Run(Box::default()));
}

fn rust_parse(list: &[String], posix: bool) -> String {
    let parsed = parse(list, posix);
    let mut text: String = parsed.warnings.concat();
    match parsed.outcome {
        Outcome::Help => text.push_str("HELP\n"),
        Outcome::Invalid | Outcome::Run(_) => {
            let options = match &parsed.outcome {
                Outcome::Run(options) => (**options).clone(),
                _ => Options::default(),
            };
            // After an error Perl still holds the values it stored; only valid runs are
            // compared in full.
            if matches!(parsed.outcome, Outcome::Run(_)) {
                let mut fields: Vec<(&str, String)> = vec![
                    ("bootdir", options.bootdir.clone()),
                    ("config", options.config.clone()),
                ];
                let optional = [
                    ("cmdline", &options.cmdline),
                    ("confd", &options.confd),
                    ("kernel", &options.kernel),
                    ("kernel_prefix", &options.kernel_prefix),
                    ("kernel_version", &options.kernel_version),
                    ("version", &options.version),
                ];
                for (key, value) in optional {
                    if let Some(value) = value {
                        fields.push((key, value.clone()));
                    }
                }
                if let Some(hooks) = &options.hookd {
                    fields.push(("cpio_hookd", hooks.join("|")));
                }
                for (key, set) in [
                    ("debug", options.debug),
                    ("disable", options.disable),
                    ("enable", options.enable),
                    ("showver", options.showver),
                ] {
                    if set {
                        fields.push((key, "1".to_owned()));
                    }
                }
                if let Some(value) = options.initcpio {
                    fields.push(("usecpio", u8::from(value).to_string()));
                }
                fields.sort();
                for (key, value) in fields {
                    text.push_str(&format!("{key}={value}\n"));
                }
                text.push_str("ok\n");
            } else {
                text.push_str("invalid\n");
            }
        }
    }
    text
}

#[test]
fn matches_getopt_long() {
    let mut parity = parity!("matches_getopt_long");
    // The results of the Perl program's `GetOptions` were recorded with
    // seed 10; another seed gives inputs that were never recorded.
    if parity.mode() == Mode::Live {
        return;
    }
    let pieces = [
        "-v",
        "--version",
        "--ver",
        "-k",
        "--kernel=x",
        "--k",
        "--ke",
        "--con",
        "--conf",
        "--confi",
        "-c",
        "--config=",
        "-C=c",
        "--noi",
        "--no-initcpio",
        "--noinit",
        "--init",
        "-i",
        "--n",
        "--no",
        "-H",
        "--hookd=a",
        "--ho",
        "--debug",
        "-d",
        "-de",
        "-dV",
        "--debug=1",
        "+d",
        "+",
        "-",
        "--",
        "---x",
        "x",
        "-x",
        "--x",
        "--en",
        "--e",
        "--enable",
        "--disable",
        "--dis",
        "--di",
        "-V",
        "--showver",
        "--sh",
        "-K",
        "--kv",
        "-p",
        "--pre",
        "-b",
        "--boot",
        "-l",
        "--cm",
        "a",
        "",
        "a=b",
        "--=x",
        "--Kver",
        "--h",
        "--help",
        "-noinitcpio",
        "--initcpio=1",
    ];
    let mut rng = Rng::from_env(10);
    for _ in 0..300 {
        let count = rng.below(6);
        let list: Vec<String> = (0..count)
            .map(|_| (*rng.choose(&pieces)).to_owned())
            .collect();
        let posix = rng.below(5) == 0;
        let legacy: String = parity.recorded(format!("args {list:?}, posix {posix}"));
        assert_eq!(
            rust_parse(&list, posix),
            legacy,
            "args {list:?}, posix {posix}"
        );
    }
}
