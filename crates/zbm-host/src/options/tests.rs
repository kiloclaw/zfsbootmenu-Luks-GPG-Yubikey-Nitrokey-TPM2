use pretty_assertions::assert_eq;

use super::*;

const SPECS: &[Spec] = &[
    Spec::value(Some("version"), Some('v')),
    Spec::value(Some("kernel"), Some('k')),
    Spec::value(Some("kver"), Some('K')),
    Spec::negatable("initcpio", Some('i')),
    Spec::flag(Some("debug"), Some('d')),
    Spec::flag(Some("enable"), None),
    Spec::value(None, Some('a')),
    Spec::flag(None, Some('e')),
];

fn args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn parsed(order: Order, given: &[&str]) -> Parsed {
    parse("tool", SPECS, order, &args(given))
}

fn given(index: usize, value: Option<&str>, negated: bool) -> Given {
    Given {
        index,
        value: value.map(str::to_owned),
        negated,
    }
}

#[test]
fn the_documented_spellings_are_read_without_a_word() {
    let read = parsed(
        Order::Permute,
        &[
            "--version=3.1",
            "-k",
            "/boot/vmlinuz",
            "op",
            "--no-initcpio",
            "-d",
            "-eafoo",
            "--kver",
            "-x",
        ],
    );

    assert_eq!(
        read.given,
        [
            given(0, Some("3.1"), false),
            given(1, Some("/boot/vmlinuz"), false),
            given(3, None, true),
            given(4, None, false),
            given(7, None, false),
            given(6, Some("foo"), false),
            given(2, Some("-x"), false),
        ]
    );
    assert_eq!(read.operands, ["op"]);
    assert!(
        read.warnings.is_empty() && read.problems.is_empty(),
        "{read:?}"
    );
}

#[test]
fn getopts_order_stops_at_the_first_operand_and_dashes_end_the_options() {
    let read = parsed(Order::StopAtOperand, &["-e", "dataset", "-d"]);
    assert_eq!(read.given, [given(7, None, false)]);
    assert_eq!(read.operands, ["dataset", "-d"]);
    let ended = parsed(Order::Permute, &["-d", "--", "-e", "x"]);
    assert_eq!(ended.given, [given(4, None, false)]);
    assert_eq!(ended.operands, ["-e", "x"]);
    assert_eq!(parsed(Order::Permute, &["-"]).operands, ["-"]);
}

#[test]
fn getopt_long_spellings_still_work_with_a_warning() {
    let read = parsed(
        Order::Permute,
        &[
            "--ver",
            "3",
            "-kernel",
            "/k",
            "+debug",
            "--noinitcpio",
            "-kver=6.1",
            "--ena",
        ],
    );

    assert_eq!(
        read.given,
        [
            given(0, Some("3"), false),
            given(1, Some("/k"), false),
            given(4, None, false),
            given(3, None, true),
            given(2, Some("6.1"), false),
            given(5, None, false),
        ]
    );
    assert!(read.problems.is_empty(), "{read:?}");
    let warned: Vec<&str> = read
        .warnings
        .iter()
        .map(|warning| warning.split(" is not how").next().unwrap_or_default())
        .collect();
    assert_eq!(
        warned,
        [
            "tool: --ver",
            "tool: -kernel",
            "tool: +debug",
            "tool: --noinitcpio",
            "tool: -kver",
            "tool: --ena"
        ]
    );
    assert!(read.warnings[0].contains("use --version, as --ver stops working in the next release"));
    assert!(read.warnings[3].contains("use --no-initcpio"));
}

#[test]
fn what_is_not_an_option_is_a_problem() {
    let read = parsed(
        Order::Permute,
        &["--bogus", "-x", "--debug=1", "--k", "--", "-k"],
    );
    assert_eq!(
        read.problems,
        [
            Problem::Unknown("bogus".to_owned()),
            Problem::Unknown("x".to_owned()),
            Problem::UnwantedValue("--debug".to_owned()),
            Problem::Ambiguous(
                "k".to_owned(),
                vec!["--kernel".to_owned(), "--kver".to_owned()]
            ),
        ]
    );
    assert_eq!(
        parsed(Order::Permute, &["--kernel"]).problems,
        [Problem::MissingValue("--kernel".to_owned())]
    );
    assert_eq!(
        parsed(Order::Permute, &["-a"]).problems,
        [Problem::MissingValue("-a".to_owned())]
    );
    assert_eq!(
        parsed(Order::Permute, &["--"]).problems,
        Vec::<Problem>::new()
    );
    assert_eq!(
        parsed(Order::Permute, &["--=x"]).problems,
        [Problem::Unknown("=x".to_owned())]
    );
    assert_eq!(
        parse(
            "t",
            &[Spec::flag(None, Some('e'))],
            Order::Permute,
            &args(&["+e"])
        )
        .operands,
        ["+e"]
    );
    assert_eq!(Spec::flag(None, None).spelling(), "");
}

#[test]
fn getopts_order_stops_at_the_first_problem() {
    let read = parsed(Order::StopAtOperand, &["-e", "-x", "-d"]);
    assert_eq!(read.given, [given(7, None, false)]);
    assert_eq!(read.problems, [Problem::Unknown("x".to_owned())]);
    // A group of short options is also a prefix of a long name: the long one
    // wins, as Getopt::Long read it, with a warning
    let read = parsed(Order::Permute, &["-de"]);
    assert_eq!(read.given, [given(4, None, false)]);
    assert_eq!(read.warnings.len(), 1);
}
