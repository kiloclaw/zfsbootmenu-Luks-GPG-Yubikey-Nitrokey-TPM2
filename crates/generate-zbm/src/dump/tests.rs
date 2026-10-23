//! Tests of the configuration writer.

use super::*;
use crate::config::load;
use zbm_testkit::legacy::perl;
use zbm_testkit::{Rng, TempDir, have_program, parity};

#[test]
fn the_default_configuration() {
    let config = load(
        "Global:\n  ManageImages: false\n  BootMountPoint: /boot/efi\n\
         Components:\n  Versions: 3\n  Enabled: true\nEFI:\n  Versions: false\n\
         Kernel:\n  CommandLine: ro quiet loglevel=0\n",
    )
    .unwrap();
    config
        .set("Global", "ManageImages", Value::Bool(true))
        .unwrap();
    assert_eq!(
        dump(&config),
        "Components:\n  Enabled: true\n  Versions: 3\nEFI:\n  Versions: false\n\
         Global:\n  BootMountPoint: /boot/efi\n  ManageImages: true\n\
         Kernel:\n  CommandLine: ro quiet loglevel=0\n"
    );
}

#[test]
fn scalars() {
    let cases = [
        ("", "''"),
        ("a: b", "'a: b'"),
        ("'q", "\"'q\""),
        ("quote'", "quote'"),
        ("123", "'123'"),
        ("x\n", "|\n    x\n"),
        ("a\tb", "\"a\\tb\""),
        ("-a", "-a"),
        ("- ", "'- '"),
    ];
    for (value, expected) in cases {
        let config = Value::map(vec![(
            "k".to_owned(),
            Value::map(vec![("v".to_owned(), Value::Str(value.to_owned()))]),
        )]);
        let written = dump(&config);
        let expected = if expected.starts_with('|') {
            format!("k:\n  v: {expected}")
        } else {
            format!("k:\n  v: {expected}\n")
        };
        assert_eq!(written, expected, "value {value:?}");
    }
    assert_eq!(represent_float(1000.0).as_deref(), Some("1000.0"));
    assert_eq!(represent_float(1e20).as_deref(), Some("1e+20.0"));
    assert_eq!(represent_float(1e15), None);
    assert_eq!(represent_float(3.1).as_deref(), Some("3.1"));
    assert_eq!(represent_float(-0.0).as_deref(), Some("0.0"));
}

/// What YAML::PP writes for a configuration after `--enable`.
fn perl_dump(yaml: &str) -> Option<String> {
    let dir = TempDir::new("generate-zbm-dump");
    let input = dir.write("in.yaml", yaml);
    let output = dir.join("out.yaml");
    let program = r#"
            use strict; use YAML::PP; use boolean; no warnings;
            my $y = eval { YAML::PP->new(boolean => "boolean")->load_file($ARGV[0]) };
            exit 3 unless defined $y and ref $y eq "HASH";
            my %config = %$y;
            $config{Global}{ManageImages} = boolean(1);
            YAML::PP->new(boolean => "boolean", header => 0)->dump_file($ARGV[1], \%config);
        "#;
    let result = perl(
        program,
        &[&input.to_string_lossy(), &output.to_string_lossy()],
        b"",
    );
    result
        .status
        .is_success()
        .then(|| std::fs::read_to_string(&output).unwrap())
}

fn rust_dump(yaml: &str) -> Option<String> {
    let config = load(yaml).ok()?;
    config
        .set("Global", "ManageImages", Value::Bool(true))
        .ok()?;
    Some(dump(&config))
}

#[test]
fn matches_yaml_pp() {
    let mut parity = parity!("matches_yaml_pp");
    if parity.mode().needs_legacy() && !have_program("perl") {
        eprintln!("skipped: perl is missing");
        return;
    }
    let scalars = [
        "~",
        "true",
        "no",
        "0",
        "-12",
        "007",
        "0o17",
        "0x1F",
        "3.10",
        "1e3",
        "1e15",
        "1e20",
        "-0.0",
        "1.5e-7",
        ".inf",
        "-.inf",
        ".nan",
        "99999999999999999999",
        "18446744073709551615",
        "abc",
        "'123'",
        "''",
        "' lead'",
        "'trail '",
        "'a: b'",
        "'#c'",
        "'x #y'",
        "'true'",
        "\"multi\\nline\"",
        "\"two\\n\\n\"",
        "\" sp\\nx\"",
        "\"tab\\there\"",
        "\"quote'\"",
        "'dq\"'",
        "'-'",
        "'- x'",
        "'?'",
        "'@at'",
        "'%p'",
        "'*star'",
        "'[br'",
        "\"ünï\"",
        "\"\\u00a0x\"",
        "\"\\ufeffx\"",
        "\"bell\\a\"",
        "'a\\b'",
        "'1.0'",
        "'0x10'",
        "1_000",
        "Yes",
        "'~'",
        "'--- x'",
        "'...'",
        "'a:'",
        "/boot/efi",
        "ro quiet loglevel=0",
        "[]",
        "{}",
        "[a, b]",
        "{x: 1, y: [2]}",
        "[[p, q], {r: s}]",
    ];
    let keys = ["a", "B", "123", "'key: x'", "''", "Global", "z"];
    let mut rng = Rng::from_env(9);
    for _ in 0..120 {
        let mut yaml = String::new();
        let mut anchored = Vec::new();
        for (index, key) in keys.iter().enumerate() {
            if rng.below(2) == 0 {
                continue;
            }
            let value = if !anchored.is_empty() && rng.below(5) == 0 {
                format!("*{}", rng.choose(&anchored))
            } else if rng.below(6) == 0 {
                anchored.push(format!("n{index}"));
                format!("&n{index} {{k{index}: {}}}", rng.choose(&scalars))
            } else if rng.below(4) == 0 {
                format!(
                    "\n  s: {}\n  t: {}",
                    rng.choose(&scalars),
                    rng.choose(&scalars)
                )
            } else {
                (*rng.choose(&scalars)).to_owned()
            };
            yaml.push_str(&format!("{key}: {value}\n"));
        }
        let legacy: Option<String> = parity.legacy(format!("{yaml:?}"), || perl_dump(&yaml));
        assert_eq!(rust_dump(&yaml), legacy, "yaml {yaml:?}");
    }
}
