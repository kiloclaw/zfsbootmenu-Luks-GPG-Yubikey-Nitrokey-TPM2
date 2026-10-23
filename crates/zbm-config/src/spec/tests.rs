use std::collections::BTreeSet;

use super::*;
use crate::args::ZbmArgs;

/// The name and the value of an example, as its source writes them.
fn split(spec: &OptionSpec, example: &str) -> (String, Option<String>) {
    match spec.source {
        Source::KernelCommandLine => {
            let name = example.split_once('=').map_or(example, |(name, _)| name);
            // Read as the start-up reads the command line: a name alone is 1.
            let value = ZbmArgs::from_cmdline(example).get(name).map(str::to_owned);
            (name.to_owned(), value)
        }
        Source::GenerateZbm(section) => {
            let (key, value) = example.split_once(": ").unwrap();
            (
                format!("{}.{key}", section.as_str()),
                Some(value.to_owned()),
            )
        }
        Source::BuildOption | Source::PoolProperty | Source::DatasetProperty | Source::Crypttab => {
            match example.split_once('=') {
                Some((name, value)) => (name.to_owned(), Some(value.trim_matches('"').to_owned())),
                None => (example.to_owned(), None),
            }
        }
    }
}

#[test]
fn every_example_is_read_as_its_option() {
    for spec in all() {
        assert!(!spec.examples.is_empty(), "{} has no example", spec.name);
        for example in spec.examples {
            let (name, value) = split(spec, example);
            let found = find(spec.source, &name);
            assert!(
                found.is_some_and(|found| std::ptr::eq(found, spec)),
                "{example:?} is not read as {}",
                spec.name
            );
            let value = value.as_deref();
            let accepted = match spec.value {
                // The command line gives a name alone the value 1.
                Value::Flag if spec.source == Source::KernelCommandLine => value == Some("1"),
                Value::Integer if spec.source == Source::GenerateZbm(GenerateSection::Global) => {
                    true
                }
                kind => kind.allows(value),
            };
            assert!(
                accepted,
                "{example:?}: {value:?} is not a value of {}",
                spec.name
            );
        }
    }
}

#[test]
fn each_name_belongs_to_one_option_of_its_source() {
    let mut seen = BTreeSet::new();
    for spec in all() {
        for name in spec.names() {
            assert!(
                seen.insert((format!("{:?}", spec.source), name)),
                "{name} is used twice"
            );
        }
    }
}

#[test]
fn options_documented_within_others_name_an_entry() {
    for spec in all() {
        if let Doc::Within(name) = spec.doc {
            let host = find(spec.source, name).unwrap();
            assert!(
                matches!(host.doc, Doc::Entry { .. }),
                "{} is documented within {name}, which has no entry",
                spec.name
            );
        }
    }
}

#[test]
fn summaries_are_one_short_line() {
    for spec in all() {
        let summary = spec.summary;
        assert!(!summary.is_empty() && summary.len() <= 80, "{}", spec.name);
        assert!(
            !summary.ends_with('.') && !summary.contains('\n'),
            "{}",
            spec.name
        );
    }
}

#[test]
fn defaults_are_values_of_their_option() {
    for spec in all() {
        if let Some(default) = spec.default {
            let accepted = match spec.value {
                Value::Bool => ["0", "1", "yes", "no", "true", "false"].contains(&default),
                kind => kind.allows(Some(default)),
            };
            assert!(accepted, "{}: default {default}", spec.name);
        }
    }
}

#[test]
fn values_accept_what_they_document() {
    assert!(Value::Flag.allows(None));
    assert!(!Value::Flag.allows(Some("1")));
    assert!(Value::Integer.allows(Some("-1")));
    assert!(Value::Integer.allows(Some("+3")));
    assert!(!Value::Integer.allows(Some("x")));
    assert!(!Value::Integer.allows(Some("-")));
    assert!(!Value::Integer.allows(None));
    assert!(Value::Choice(&["a", "b"]).allows(Some("b")));
    assert!(!Value::Choice(&["a", "b"]).allows(Some("c")));
    assert!(Value::FlagOrChoice(&["1"]).allows(None));
    assert!(Value::FlagOrChoice(&["1"]).allows(Some("1")));
    assert!(!Value::Text.allows(None));
    assert!(Value::Bool.allows(Some("off")));
}

#[test]
fn options_are_found_by_name_and_alias_in_their_source() {
    assert_eq!(
        find(Source::KernelCommandLine, "spl_hostid").unwrap().name,
        "spl.spl_hostid"
    );
    assert_eq!(find(Source::Crypttab, "keyslot").unwrap().name, "key-slot");
    assert!(find(Source::Crypttab, "zbm.timeout").is_none());
    let versions = find(Source::GenerateZbm(GenerateSection::Efi), "EFI.Versions").unwrap();
    assert_eq!(versions.name, "EFI.Versions");
    assert_eq!(GenerateSection::Components.as_str(), "Components");
    assert_eq!(
        cmdline::TIMEOUT.names().collect::<Vec<_>>(),
        ["zbm.timeout", "timeout"]
    );
    assert!(cmdline::TIMEOUT.alias("zbm.timeout").is_none());
}
