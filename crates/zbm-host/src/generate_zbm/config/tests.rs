//! Tests of the configuration loader.

use super::*;
use zbm_testkit::legacy::perl;
use zbm_testkit::{Rng, TempDir, can_run, have_program, parity};

#[test]
fn plain_scalars_follow_the_core_schema() {
    assert_eq!(resolve_plain(""), Value::Null);
    assert_eq!(resolve_plain("~"), Value::Null);
    assert_eq!(resolve_plain("True"), Value::Bool(true));
    assert_eq!(resolve_plain("FALSE"), Value::Bool(false));
    assert_eq!(resolve_plain("007"), Value::Int(7));
    assert_eq!(resolve_plain("-0"), Value::Int(0));
    assert_eq!(resolve_plain("0o17"), Value::Int(15));
    assert_eq!(resolve_plain("0x10"), Value::Int(16));
    assert_eq!(resolve_plain("3.10"), Value::Float(3.1));
    assert_eq!(resolve_plain("1e3"), Value::Float(1000.0));
    assert_eq!(resolve_plain(".5"), Value::Float(0.5));
    assert_eq!(resolve_plain("5."), Value::Float(5.0));
    assert_eq!(resolve_plain("7.3.*"), Value::Str("7.3.*".to_owned()));
    assert_eq!(resolve_plain("1e"), Value::Str("1e".to_owned()));
    assert_eq!(resolve_plain("0x"), Value::Str("0x".to_owned()));
}

#[test]
fn numbers_print_like_perl() {
    assert_eq!(format_g15(3.1), "3.1");
    assert_eq!(format_g15(1000.0), "1000");
    assert_eq!(format_g15(1e20), "1e+20");
    assert_eq!(format_g15(1e-6), "1e-06");
    assert_eq!(format_g15(1.5e-7), "1.5e-07");
    assert_eq!(format_g15(0.0001), "0.0001");
    assert_eq!(format_g15(0.5), "0.5");
    assert_eq!(format_g15(123456789012345.0), "123456789012345");
    assert_eq!(format_g15(1234567890123456.0), "1.23456789012346e+15");
    assert_eq!(format_g15(f64::INFINITY), "Inf");
    assert_eq!(format_g15(f64::NAN), "NaN");
}

#[test]
fn perl_rules() {
    assert!(Value::Bool(false).has_value());
    assert!(!Value::Bool(false).truthy());
    assert!(!Value::Str(String::new()).has_value());
    assert!(Value::Str("0.0".to_owned()).truthy());
    assert!(!Value::Str("0".to_owned()).truthy());
    assert_eq!(Value::Str("3abc".to_owned()).perl_int(), 3);
    assert_eq!(Value::Str("1e2".to_owned()).perl_int(), 100);
    assert_eq!(Value::Str("-4.7".to_owned()).perl_int(), -4);
    assert_eq!(Value::Str("0x10".to_owned()).perl_int(), 0);
    assert_eq!(Value::Str(" 12".to_owned()).perl_int(), 12);
    assert_eq!(Value::Bool(true).perl_int(), 1);
    assert_eq!(Value::Float(3.9).perl_int(), 3);
}

#[test]
fn documents() {
    assert!(load("").is_err());
    assert!(load("- a\n").is_err());
    assert!(load("just text\n").is_err());
    assert!(load("a: 1\na: 2\n").is_err());
    let config = load("a: &x 1\nb: *x\n---\nc: 2\n").unwrap();
    assert_eq!(config.get("b"), Some(Value::Int(1)));
    assert_eq!(config.get("c"), None);
    let config =
        load("a: !!str 3\nb: !!int \"4\"\nc: !foo bar\nd: 'q'\ne: >-\n  folded\n  text\n").unwrap();
    assert_eq!(config.get("a"), Some(Value::Str("3".to_owned())));
    assert_eq!(config.get("b"), Some(Value::Int(4)));
    assert_eq!(config.get("c"), Some(Value::Str("bar".to_owned())));
    assert_eq!(config.get("d"), Some(Value::Str("q".to_owned())));
    assert_eq!(config.get("e"), Some(Value::Str("folded text".to_owned())));
}

#[test]
fn sections() {
    let config = load("Global:\n  ManageImages: true\nEFI: text\n").unwrap();
    assert_eq!(
        config.lookup("Global", "ManageImages").unwrap(),
        Some(Value::Bool(true))
    );
    assert_eq!(config.lookup("Kernel", "Path").unwrap(), None);
    assert!(config.lookup("EFI", "Enabled").is_err());
    config
        .set("Components", "Enabled", Value::Bool(false))
        .unwrap();
    assert_eq!(
        config.lookup("Components", "Enabled").unwrap(),
        Some(Value::Bool(false))
    );
}

#[test]
fn aliases_share_collections() {
    let config =
        load("Other: &g\n  BootMountPoint: /boot\nGlobal: *g\nList: &l [1]\nCopy: *l\n").unwrap();
    config
        .set("Global", "ManageImages", Value::Bool(true))
        .unwrap();
    assert_eq!(
        config.lookup("Other", "ManageImages").unwrap(),
        Some(Value::Bool(true))
    );
    assert!(load("a: &x [1, *x]\n").is_err());
    let config = load("Global:\n").unwrap();
    config
        .set("Global", "ManageImages", Value::Bool(false))
        .unwrap();
    assert_eq!(
        config.lookup("Global", "ManageImages").unwrap(),
        Some(Value::Bool(false))
    );
}

/// For each key of a generated mapping, what Perl sees: defined, printed form,
/// truth and `int`.
fn perl_view(yaml: &str) -> Option<String> {
    let dir = TempDir::new("generate-zbm-config");
    let file = dir.write("config.yaml", yaml);
    let program = r#"
            use YAML::PP; use boolean; no warnings;
            my $y = eval { YAML::PP->new(boolean => "boolean")->load_file($ARGV[0]) };
            exit 3 unless defined $y and ref $y eq "HASH";
            for my $k (sort keys %$y) {
                my $v = $y->{$k};
                if (ref $v eq "ARRAY" or ref $v eq "HASH") { print "$k|ref\n"; next; }
                my $i = int($v); $i = "big" if $i != $i or abs($i) >= 1e15;
                printf "%s|%s|%s|%s|%s\n", $k, (defined $v ? 1 : 0),
                    (defined $v ? "$v" : ""), ($v ? 1 : 0), $i;
            }
        "#;
    let output = perl(program, &[&file.to_string_lossy()], b"");
    output.status.is_success().then(|| output.stdout_text())
}

fn rust_view(yaml: &str) -> Option<String> {
    let Ok(Value::Map(map)) = load(yaml) else {
        return None;
    };
    let mut entries = map.borrow().clone();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Some(
        entries
            .iter()
            .map(|(key, value)| match value {
                Value::Seq(_) | Value::Map(_) => format!("{key}|ref\n"),
                value => {
                    // Integers beyond 15 digits and NaN are not meaningful configuration
                    // values; Perl prints them in other forms, so only their kind is compared.
                    let int = value.perl_int();
                    let nan = matches!(value, Value::Float(number) if number.is_nan());
                    let int = if nan || int.unsigned_abs() >= 1_000_000_000_000_000 {
                        "big".to_owned()
                    } else {
                        int.to_string()
                    };
                    format!(
                        "{key}|{}|{}|{}|{int}\n",
                        u8::from(value.is_defined()),
                        value.perl_string().unwrap_or_default(),
                        u8::from(value.truthy()),
                    )
                }
            })
            .collect(),
    )
}

#[test]
fn values_match_yaml_pp() {
    let mut parity = parity!("values_match_yaml_pp");
    if !can_run(
        !parity.mode().needs_legacy() || have_program("perl"),
        "perl",
    ) {
        return;
    }
    let scalars = [
        "",
        "~",
        "null",
        "NULL",
        "true",
        "True",
        "false",
        "FALSE",
        "yes",
        "no",
        "0",
        "1",
        "3",
        "007",
        "-0",
        "+5",
        "0o17",
        "0x1F",
        "0x",
        "3.10",
        "1e3",
        ".5",
        "5.",
        "1.0",
        "-4.7",
        "1e20",
        "1e-6",
        ".inf",
        "-.Inf",
        ".nan",
        "7.3.*",
        "6.1",
        "abc",
        "3abc",
        "'0'",
        "\"\"",
        "'007'",
        "\"1e3\"",
        "!!str 12",
        "!!int \"9\"",
        "[a, b]",
        "{x: 1}",
        "/boot/efi",
        "quiet loglevel=4",
        "12345678901234567890",
        "99999999999999999999",
    ];
    let mut rng = Rng::from_env(8);
    for _ in 0..60 {
        let count = 1 + rng.below(6);
        let yaml: String = (0..count)
            .map(|index| format!("k{index}: {}\n", rng.choose(&scalars)))
            .collect();
        let legacy: Option<String> = parity.legacy(format!("{yaml:?}"), || perl_view(&yaml));
        assert_eq!(rust_view(&yaml), legacy, "yaml {yaml:?}");
    }
}
