use super::*;
use scan::{FileScan, FnScan, Role};
use std::path::PathBuf;

fn function(name: &str, lines: usize, returns: Returns) -> FnScan {
    FnScan {
        name: name.to_owned(),
        lines,
        returns,
        ..FnScan::default()
    }
}

fn test_fn(name: &str, runs_legacy: bool, mentions: &[&str]) -> FnScan {
    FnScan {
        name: name.to_owned(),
        role: Role::Test,
        runs_legacy,
        mentions: mentions.iter().map(|m| (*m).to_owned()).collect(),
        ..FnScan::default()
    }
}

fn file(lines: usize, functions: Vec<FnScan>) -> ScannedFile {
    ScannedFile {
        path: PathBuf::from("src/lib.rs"),
        test_code: false,
        scan: FileScan {
            lines,
            functions,
            ..FileScan::default()
        },
    }
}

#[test]
fn long_functions_and_files_are_counted_past_the_limits() {
    let files = [file(
        601,
        vec![
            function("a", 60, Returns::Other),
            function("b", 61, Returns::Other),
        ],
    )];

    let counts = count(&files);

    assert_eq!(counts.long_functions, 1);
    assert_eq!(counts.long_files, 1);
    assert_eq!(counts.functions, 2);
}

#[test]
fn predicates_and_trait_methods_are_not_flag_functions() {
    let mut eq = function("eq", 3, Returns::Bool);
    eq.trait_impl = true;
    let files = [file(
        10,
        vec![
            function("unlock", 3, Returns::Bool),
            function("cryptsetup_open", 3, Returns::Bool),
            function("is_open", 3, Returns::Bool),
            function("tpm2_device_present", 3, Returns::Bool),
            function("import", 3, Returns::Result),
            eq,
        ],
    )];

    let counts = count(&files);

    assert_eq!(counts.flag_functions, 2);
    assert_eq!(counts.result_functions, 1);
}

#[test]
fn tests_that_reach_legacy_code_through_helpers_are_counted() {
    let mut helper = test_fn("legacy_parse", true, &[]);
    helper.role = Role::TestCode;
    let files = [file(
        10,
        vec![
            helper,
            test_fn("matches_legacy", false, &["legacy_parse"]),
            test_fn("direct", true, &[]),
            test_fn("pure", false, &["parse"]),
        ],
    )];

    assert_eq!(count(&files).legacy_tests, 2);
    assert_eq!(count(&files).tests, 3);
}

#[test]
fn a_rise_above_the_baseline_fails_and_new_crates_must_be_clean() {
    let mut all = BTreeMap::new();
    all.insert(
        "old".to_owned(),
        Counts {
            long_functions: 3,
            ..Counts::default()
        },
    );
    all.insert(
        "new".to_owned(),
        Counts {
            unsafe_blocks: 1,
            ..Counts::default()
        },
    );
    let baseline = baseline::parse("crate long_functions\nold 2\n").unwrap();

    let error = check(&all, &baseline).unwrap_err();

    assert_eq!(
        error.to_string(),
        "metrics found 2 problem(s)\n  new: unsafe_blocks rose from 0 to 1\n  \
         old: long_functions rose from 2 to 3"
    );
}

#[test]
fn the_table_has_a_total_row() {
    let mut all = BTreeMap::new();
    all.insert(
        "a".to_owned(),
        Counts {
            files: 2,
            ..Counts::default()
        },
    );
    all.insert(
        "b".to_owned(),
        Counts {
            files: 3,
            ..Counts::default()
        },
    );

    let text = table(&all);

    let total = text.lines().last().unwrap();
    assert!(total.starts_with("total"));
    assert_eq!(total.split_whitespace().nth(1), Some("5"));
}
