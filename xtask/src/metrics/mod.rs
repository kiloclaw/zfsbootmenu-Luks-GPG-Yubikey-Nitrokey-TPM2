//! Code metrics that show how far the refactoring has come.
//!
//! `cargo xtask metrics` scans every crate (see [`tree`] and [`scan`]) and
//! prints, per crate, the numbers that REFACTOR-ROADMAP.md's findings are
//! about. The *debt* metrics may only go down: `--check` compares them with
//! the committed baseline (`xtask/metrics-baseline.txt`, see [`baseline`])
//! and fails when a crate has more than before; `--bless` writes the current
//! numbers as the new baseline.

pub mod baseline;
pub mod legacy;
pub mod scan;
pub mod tree;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::error::{Error, Result};
use crate::process;
use crate::workspace::Workspace;
use scan::Returns;
use tree::ScannedFile;

/// Functions longer than this many lines count as long.
pub const LONG_FUNCTION: usize = 60;

/// Files longer than this many lines count as long.
pub const LONG_FILE: usize = 600;

/// The baseline file, relative to the workspace root.
pub const BASELINE: &str = "xtask/metrics-baseline.txt";

/// Words that make a function name a question, for which returning `bool`
/// is right: `is_open`, `tpm2_device_present`, `have_program`.
const PREDICATE_WORDS: &[&str] = &[
    "allowed",
    "allows",
    "available",
    "can",
    "contains",
    "enabled",
    "ends",
    "equals",
    "exists",
    "fits",
    "has",
    "have",
    "ignores",
    "in",
    "is",
    "matches",
    "may",
    "mentions",
    "merges",
    "must",
    "names",
    "needs",
    "permits",
    "present",
    "reads",
    "refers",
    "requires",
    "running",
    "should",
    "starts",
    "succeeds",
    "supports",
    "truthy",
    "valid",
    "wants",
    "was",
    "were",
    "will",
];

/// The numbers of one crate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Counts {
    /// Source files.
    pub files: usize,
    /// Lines of all source files.
    pub lines: usize,
    /// Functions outside tests.
    pub functions: usize,
    /// `#[test]` functions.
    pub tests: usize,
    /// Functions outside tests longer than [`LONG_FUNCTION`] lines.
    pub long_functions: usize,
    /// Files longer than [`LONG_FILE`] lines.
    pub long_files: usize,
    /// Functions outside tests that return `bool` but are not predicates:
    /// success flags that lose the reason for a failure.
    pub flag_functions: usize,
    /// Functions outside tests that return a `Result`.
    pub result_functions: usize,
    /// Doc comment lines that explain the code by a legacy function.
    pub legacy_docs: usize,
    /// Uses of the shell-semantics helpers outside tests.
    pub shell_helpers: usize,
    /// Tests that run legacy Bash or Perl code.
    pub legacy_tests: usize,
    /// `unsafe` blocks.
    pub unsafe_blocks: usize,
}

/// One metric: how to read it from [`Counts`] and whether it is debt.
#[derive(Clone, Copy, Debug)]
pub struct Metric {
    /// The key in the baseline and in `metrics.json`.
    pub key: &'static str,
    /// The column heading in the printed table.
    pub heading: &'static str,
    /// Whether the metric may only go down.
    pub debt: bool,
    /// Reads the metric.
    pub get: fn(&Counts) -> usize,
}

/// Every metric, in the order of the printed table.
pub const METRICS: &[Metric] = &[
    metric("files", "files", false, |c| c.files),
    metric("lines", "lines", false, |c| c.lines),
    metric("functions", "fns", false, |c| c.functions),
    metric("tests", "tests", false, |c| c.tests),
    metric("long_functions", "long fns", true, |c| c.long_functions),
    metric("long_files", "long files", true, |c| c.long_files),
    metric("flag_functions", "bool flags", true, |c| c.flag_functions),
    metric("result_functions", "Result fns", false, |c| {
        c.result_functions
    }),
    metric("legacy_docs", "legacy docs", true, |c| c.legacy_docs),
    metric("shell_helpers", "shell helpers", true, |c| c.shell_helpers),
    metric("legacy_tests", "legacy tests", true, |c| c.legacy_tests),
    metric("unsafe_blocks", "unsafe", true, |c| c.unsafe_blocks),
];

/// A [`Metric`]; a function so that the table above stays short.
const fn metric(
    key: &'static str,
    heading: &'static str,
    debt: bool,
    get: fn(&Counts) -> usize,
) -> Metric {
    Metric {
        key,
        heading,
        debt,
        get,
    }
}

/// The keys of the debt metrics.
pub fn debt_keys() -> Vec<&'static str> {
    METRICS.iter().filter(|m| m.debt).map(|m| m.key).collect()
}

/// Whether a function called `name` answers a question: one of the words
/// of its name is in [`PREDICATE_WORDS`], or it is `eq` or `ne`.
pub fn is_predicate_name(name: &str) -> bool {
    matches!(name, "eq" | "ne") || name.split('_').any(|word| PREDICATE_WORDS.contains(&word))
}

/// The numbers of a crate made of `files`.
pub fn count(files: &[ScannedFile]) -> Counts {
    let mut counts = Counts {
        files: files.len(),
        ..Counts::default()
    };
    for file in files {
        counts.lines += file.scan.lines;
        counts.long_files += usize::from(file.scan.lines > LONG_FILE);
        counts.legacy_docs += file.scan.legacy_docs;
        counts.shell_helpers += file.scan.shell_helpers;
        counts.unsafe_blocks += file.scan.unsafe_blocks;
        for function in &file.scan.functions {
            if function.role.is_test() {
                counts.tests += 1;
            }
            if function.role.is_test_code() {
                continue;
            }
            counts.functions += 1;
            counts.long_functions += usize::from(function.lines > LONG_FUNCTION);
            counts.result_functions += usize::from(function.returns == Returns::Result);
            let flag = function.returns == Returns::Bool
                && !function.trait_impl
                && !is_predicate_name(&function.name);
            counts.flag_functions += usize::from(flag);
        }
    }
    counts.legacy_tests = legacy_tests(files);
    counts
}

/// The number of tests that run legacy code, directly or through a helper
/// function of the crate's test code that does.
///
/// Helpers are matched by name across the crate, which can over-count when
/// two test modules have helpers of the same name; the number only has to be
/// consistent from one run to the next.
fn legacy_tests(files: &[ScannedFile]) -> usize {
    let test_fns: Vec<_> = files
        .iter()
        .flat_map(|file| &file.scan.functions)
        .filter(|function| function.role.is_test_code())
        .collect();
    let mut legacy: BTreeSet<&str> = test_fns
        .iter()
        .filter(|function| function.runs_legacy)
        .map(|function| function.name.as_str())
        .collect();
    loop {
        let before = legacy.len();
        for function in &test_fns {
            if function
                .mentions
                .iter()
                .any(|name| legacy.contains(name.as_str()))
            {
                legacy.insert(&function.name);
            }
        }
        if legacy.len() == before {
            break;
        }
    }
    test_fns
        .iter()
        .filter(|function| function.role.is_test() && legacy.contains(function.name.as_str()))
        .count()
}

/// What `cargo xtask metrics` does after printing the numbers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Nothing more.
    Report,
    /// Fail when a debt metric is above the baseline.
    Check,
    /// Write the numbers as the new baseline.
    Bless,
}

/// The numbers of every crate of `workspace`, by crate name.
///
/// # Errors
///
/// When a source file cannot be read or parsed.
pub fn measure(workspace: &Workspace) -> Result<BTreeMap<String, Counts>> {
    let legacy_names = legacy::function_names(&workspace.root.join("legacy"))?;
    let mut all = BTreeMap::new();
    for package in &workspace.packages {
        let files = tree::scan_package(package, &legacy_names)?;
        all.insert(package.name.clone(), count(&files));
    }
    Ok(all)
}

/// Runs `cargo xtask metrics`.
///
/// # Errors
///
/// [`Error::Check`] in [`Mode::Check`] when a crate went above its baseline;
/// I/O and parse errors otherwise.
pub fn run(workspace: &Workspace, mode: Mode) -> Result<()> {
    let all = measure(workspace)?;
    print!("{}", table(&all));
    write_json(workspace, &all)?;
    if process::is_github_actions() {
        process::job_summary(&markdown(&all))?;
    }
    let path = workspace.root.join(BASELINE);
    match mode {
        Mode::Report => Ok(()),
        Mode::Bless => {
            let text = baseline::format(&as_baseline(&all), baseline::DEBT_HEADER, &debt_keys());
            std::fs::write(&path, text)
                .map_err(|error| Error::io(format!("writing {BASELINE}"), error))?;
            println!("metrics: wrote {BASELINE}");
            Ok(())
        }
        Mode::Check => {
            let text = std::fs::read_to_string(&path)
                .map_err(|error| Error::io(format!("reading {BASELINE}"), error))?;
            check(&all, &baseline::parse(&text)?)
        }
    }
}

/// The debt metrics of `all` as a baseline.
fn as_baseline(all: &BTreeMap<String, Counts>) -> baseline::Baseline {
    all.iter()
        .map(|(name, counts)| {
            let metrics = METRICS
                .iter()
                .filter(|m| m.debt)
                .map(|m| (m.key.to_owned(), (m.get)(counts)))
                .collect();
            (name.clone(), metrics)
        })
        .collect()
}

/// Compares `all` with `baseline`: an error for every rise, a note for every
/// improvement that the baseline does not record yet.
///
/// # Errors
///
/// [`Error::Check`] listing every debt metric above its baseline.
pub fn check(all: &BTreeMap<String, Counts>, baseline: &baseline::Baseline) -> Result<()> {
    let mut problems = Vec::new();
    let mut improved = false;
    for (name, counts) in all {
        for metric in METRICS.iter().filter(|m| m.debt) {
            let now = (metric.get)(counts);
            let allowed = baseline
                .get(name)
                .and_then(|metrics| metrics.get(metric.key))
                .copied()
                .unwrap_or_default();
            if now > allowed {
                problems.push(format!(
                    "{name}: {} rose from {allowed} to {now}",
                    metric.key
                ));
            }
            improved |= now < allowed;
        }
    }
    if improved {
        println!("metrics: some numbers went down; run `cargo xtask metrics --bless` to keep them");
    }
    if problems.is_empty() {
        println!("metrics: no crate is above its baseline");
        Ok(())
    } else {
        Err(Error::Check {
            check: "metrics",
            problems,
        })
    }
}

/// The numbers as an aligned text table with a total row.
pub fn table(all: &BTreeMap<String, Counts>) -> String {
    let rows = rows(all);
    let mut widths: Vec<usize> = std::iter::once("crate".len())
        .chain(METRICS.iter().map(|m| m.heading.len()))
        .collect();
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.len());
        }
    }
    let header: Vec<String> = std::iter::once("crate".to_owned())
        .chain(METRICS.iter().map(|m| m.heading.to_owned()))
        .collect();
    let mut text = String::new();
    for row in std::iter::once(&header).chain(&rows) {
        for (column, (cell, width)) in row.iter().zip(&widths).enumerate() {
            let _ = if column == 0 {
                write!(text, "{cell:<width$}")
            } else {
                write!(text, "  {cell:>width$}")
            };
        }
        text.push('\n');
    }
    text
}

/// The numbers as a Markdown table for the job summary.
fn markdown(all: &BTreeMap<String, Counts>) -> String {
    let headings: Vec<&str> = METRICS.iter().map(|m| m.heading).collect();
    let mut text = format!("## Code metrics\n\n| crate | {} |\n", headings.join(" | "));
    let _ = writeln!(text, "| --- |{}", " ---: |".repeat(METRICS.len()));
    for row in rows(all) {
        let _ = writeln!(text, "| {} |", row.join(" | "));
    }
    text
}

/// One row of cells per crate and a total row.
fn rows(all: &BTreeMap<String, Counts>) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = all
        .iter()
        .map(|(name, counts)| {
            std::iter::once(name.clone())
                .chain(METRICS.iter().map(|m| (m.get)(counts).to_string()))
                .collect()
        })
        .collect();
    let total: Vec<String> = std::iter::once("total".to_owned())
        .chain(
            METRICS
                .iter()
                .map(|m| all.values().map(m.get).sum::<usize>().to_string()),
        )
        .collect();
    rows.push(total);
    rows
}

/// Writes `target/metrics.json` for other tools.
fn write_json(workspace: &Workspace, all: &BTreeMap<String, Counts>) -> Result<()> {
    let crates: serde_json::Map<String, serde_json::Value> = all
        .iter()
        .map(|(name, counts)| {
            let metrics: serde_json::Map<String, serde_json::Value> = METRICS
                .iter()
                .map(|m| (m.key.to_owned(), (m.get)(counts).into()))
                .collect();
            (name.clone(), metrics.into())
        })
        .collect();
    let json = serde_json::json!({
        "long_function_lines": LONG_FUNCTION,
        "long_file_lines": LONG_FILE,
        "crates": crates,
    });
    let dir = workspace.root.join("target");
    std::fs::create_dir_all(&dir).map_err(|error| Error::io("creating target/", error))?;
    let text = serde_json::to_string_pretty(&json)
        .map_err(|error| Error::Format(format!("metrics.json: {error}")))?;
    std::fs::write(dir.join("metrics.json"), text + "\n")
        .map_err(|error| Error::io("writing target/metrics.json", error))
}

#[cfg(test)]
mod tests {
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
}
