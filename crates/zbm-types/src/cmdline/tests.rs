use super::*;
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use zbm_testkit::cases;

/// `text` split as the kernel splits it, each argument as name and value.
fn split(text: &str) -> Vec<(String, Option<String>)> {
    let line: KernelCommandLine = text.parse().unwrap();
    line.args()
        .iter()
        .map(|arg| (arg.name().to_owned(), arg.value().map(str::to_owned)))
        .collect()
}

fn arg(name: &str, value: Option<&str>) -> (String, Option<String>) {
    (name.to_owned(), value.map(str::to_owned))
}

#[test]
fn command_lines_split_as_next_arg_splits_them() {
    cases! {
        for (text, expected) in {
            "empty" => ("", vec![]),
            "spaces only" => (" \t\n ", vec![]),
            "names and values" => (
                "quiet loglevel=4",
                vec![arg("quiet", None), arg("loglevel", Some("4"))],
            ),
            "runs of spaces and tabs" => (
                "  a\t\tb \n",
                vec![arg("a", None), arg("b", None)],
            ),
            "the first equals sign" => ("root=ZFS=tank", vec![arg("root", Some("ZFS=tank"))]),
            "an empty value" => ("a=", vec![arg("a", Some(""))]),
            "a quoted value" => (
                "dyndbg=\"file a.c +p\" x",
                vec![arg("dyndbg", Some("file a.c +p")), arg("x", None)],
            ),
            "a quoted argument" => ("\"a=b c\"", vec![arg("a", Some("b c"))]),
            "a quoted name" => ("\"quiet\"", vec![arg("quiet", None)]),
            "a lone opening quote of a value" => ("a=\"", vec![arg("a", Some(""))]),
            "an unclosed quote runs to the end" => ("a=\"b c", vec![arg("a", Some("b c"))]),
            "an equals sign first is part of no name" => ("x==y", vec![arg("x", Some("=y"))]),
        } {
            assert_eq!(split(text), expected);
        }
    }
}

#[test]
fn quotes_that_would_not_print_back_are_refused() {
    for text in ["na\"me", "\"", "=x", "a=\u{1}", "a=b\"c", "a=\"\"b\"\""] {
        assert!(text.parse::<KernelCommandLine>().is_err(), "{text:?}");
    }
}

#[test]
fn quotes_inside_a_value_stay_as_the_kernel_keeps_them() {
    // The kernel keeps these quotes, and the space between them.
    assert_eq!(
        split("a=b\"c d\" x"),
        vec![arg("a", Some("b\"c d\"")), arg("x", None)]
    );
    let line: KernelCommandLine = "a=b\"c d\"e f=\"g\"".parse().unwrap();
    assert_eq!(line.to_string(), "a=b\"c d\"e f=g");
    assert_eq!(line.to_string().parse::<KernelCommandLine>().unwrap(), line);
    assert!(KernelArg::new("a", Some("b c\"d\"")).is_err());
}

#[test]
fn a_lenient_split_keeps_what_it_can() {
    let (line, refused) = KernelCommandLine::lenient("quiet na\"me\"=1 =x  ro\n");
    assert_eq!(line.to_string(), "quiet ro");
    assert_eq!(refused, ["na\"me\"=1", "=x"]);
    // An open quote takes the rest of the line, as the kernel reads it.
    let (line, refused) = KernelCommandLine::lenient("quiet a=b\"c ro");
    assert_eq!(line.to_string(), "quiet");
    assert_eq!(refused, ["a=b\"c ro"]);
    let (line, refused) = KernelCommandLine::lenient("");
    assert!(line.is_empty() && refused.is_empty());
}

#[test]
fn arguments_are_removed_by_name_and_added() {
    let mut line: KernelCommandLine = "root=zfs:a quiet spl_hostid=1 rootwait".parse().unwrap();
    line = line.without(&["root", "spl_hostid"]);
    assert_eq!(line.to_string(), "quiet rootwait");
    line.push(KernelArg::new("spl_hostid", Some("00000000")).unwrap());
    line.extend(["noresume".parse::<KernelArg>().unwrap()]);
    assert_eq!(
        line.one_per_line(),
        "quiet\nrootwait\nspl_hostid=00000000\nnoresume\n"
    );
    assert_eq!(
        line.one_per_line().parse::<KernelCommandLine>().unwrap(),
        line
    );
    let collected: KernelCommandLine = line.args().iter().take(1).cloned().collect();
    assert_eq!(collected.to_string(), "quiet");
}

#[test]
fn one_argument_is_one() {
    assert_eq!(
        "init=/bin/sh".parse::<KernelArg>().unwrap(),
        KernelArg::new("init", Some("/bin/sh")).unwrap()
    );
    let reason = |text: &str| text.parse::<KernelArg>().unwrap_err().reason().clone();
    assert_eq!(reason("a b"), Reason::NotOneArgument);
    assert_eq!(reason(" a"), Reason::InvalidCharacter(' '));
    assert_eq!(reason(""), Reason::Empty);
}

#[test]
fn names_and_values_have_their_rules() {
    let reason =
        |name: &str, value: Option<&str>| KernelArg::new(name, value).unwrap_err().reason().clone();
    assert_eq!(reason("", None), Reason::Empty);
    assert_eq!(reason("a b", None), Reason::InvalidCharacter(' '));
    assert_eq!(reason("a=b", None), Reason::InvalidCharacter('='));
    assert_eq!(reason("a", Some("b\"")), Reason::InvalidCharacter('"'));
    assert_eq!(reason("a", Some("b\tc")), Reason::InvalidCharacter('\t'));
    assert_eq!(
        KernelArg::new("a", Some("b c")).unwrap().to_string(),
        "a=\"b c\""
    );
}

#[test]
fn the_last_argument_of_a_name_wins() {
    let line: KernelCommandLine = "a=1 b a=2 c=".parse().unwrap();
    assert_eq!(line.value("a"), Some("2"));
    assert_eq!(line.value("b"), None);
    assert_eq!(line.value("c"), Some(""));
    assert_eq!(line.value("d"), None);
}

fn kernel_arg() -> impl Strategy<Value = KernelArg> {
    let name = "[a-z_.-][a-z0-9_.:/-]{0,8}";
    let value = proptest::option::of(prop_oneof![
        "[a-zA-Z0-9 =:/,._-]{0,12}",
        "[a-z]{1,3}\"[a-z ]{0,4}\"[a-z]{0,3}",
    ]);
    (name, value).prop_map(|(name, value)| KernelArg::new(&name, value.as_deref()).unwrap())
}

proptest! {
    #[test]
    fn arguments_round_trip(arg in kernel_arg()) {
        prop_assert_eq!(arg.to_string().parse::<KernelArg>().unwrap(), arg);
    }

    #[test]
    fn command_lines_round_trip(args in proptest::collection::vec(kernel_arg(), 0..6)) {
        let line = KernelCommandLine::from(args);
        prop_assert_eq!(line.to_string().parse::<KernelCommandLine>().unwrap(), line);
    }

    #[test]
    fn any_text_parses_or_is_refused(text in "\\PC{0,40}") {
        let _ = text.parse::<KernelCommandLine>();
        let _ = text.parse::<KernelArg>();
    }
}
