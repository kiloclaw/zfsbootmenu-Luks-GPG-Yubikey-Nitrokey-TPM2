//! `cargo xtask coverage`: line coverage per crate, which may only go up.
//!
//! The tests of the workspace run under `cargo llvm-cov`, and the lines of
//! each source file count for the crate whose directory holds it, but for
//! test code (ROADMAP.md VE1): files that are tests as a whole, and inline
//! `#[cfg(test)]` modules and test functions, which the scanner of `metrics`
//! finds; the tests run them, so they would raise every number. `--check`
//! compares the whole percent of each crate with the committed baseline
//! (`xtask/coverage-baseline.txt`) and fails
//! when it dropped; a crate that is not in the baseline must reach
//! [`NEW_CRATE`]. `--bless` writes the current numbers as the new baseline.
//!
//! This is a command of its own, not part of `metrics`, because it runs
//! every test in an instrumented build; the parity tests replay their
//! recordings (`zbm_testkit::parity`), as in every test run.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::cargo;
use crate::error::{Error, Result};
use crate::metrics::{Mode, baseline, legacy, tree};
use crate::process;
use crate::workspace::Workspace;

/// The baseline file, relative to the workspace root.
pub const BASELINE: &str = "xtask/coverage-baseline.txt";

/// The line coverage, in percent, that a crate missing from the baseline
/// must reach.
pub const NEW_CRATE: usize = 80;

/// The column of the baseline.
const KEY: &str = "line_coverage";

/// The report of `cargo llvm-cov`, in LCOV's format, which counts each
/// line; relative to the workspace root.
const REPORT: &str = "target/coverage.lcov";

/// The test code of each file that holds some: all of it, or ranges of
/// lines.
pub type TestCode = HashMap<PathBuf, TestLines>;

/// The test code of one file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TestLines {
    /// Whether the whole file is test code.
    pub whole: bool,
    /// Ranges of lines of test code, first and last.
    pub ranges: Vec<(usize, usize)>,
}

impl TestLines {
    /// Whether `line` is test code.
    fn is_test(&self, line: usize) -> bool {
        self.whole
            || self
                .ranges
                .iter()
                .any(|&(first, last)| (first..=last).contains(&line))
    }
}

/// The test code of the workspace's crates, as `metrics` scans them.
///
/// # Errors
///
/// When a source file cannot be read or parsed.
pub fn test_code(workspace: &Workspace) -> Result<TestCode> {
    let legacy_names = legacy::function_names(&workspace.root.join("legacy"))?;
    let mut code = TestCode::new();
    for package in &workspace.packages {
        for file in tree::scan_package(package, &legacy_names)? {
            if file.test_code || !file.scan.test_lines.is_empty() {
                code.insert(
                    file.path.clone(),
                    TestLines {
                        whole: file.test_code,
                        ranges: file.scan.test_lines.clone(),
                    },
                );
            }
        }
    }
    Ok(code)
}

/// The header comment written above the baseline.
const HEADER: &str = "\
# The line coverage of each crate in whole percent, written by
# `cargo xtask coverage --bless`. `cargo xtask coverage --check` fails when a
# crate drops below its number; a crate that is not listed must reach 80.
# Raise a number by testing more and running the command again; never lower
# one by hand.
";

/// The lines of a crate that the tests can reach, and those they ran.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Lines {
    /// Lines with code.
    pub count: u64,
    /// Lines with code that a test ran.
    pub covered: u64,
}

impl Lines {
    /// The coverage in whole percent, rounded down; `None` without code.
    pub fn percent(self) -> Option<usize> {
        let percent = self.covered.checked_mul(100)?.checked_div(self.count)?;
        usize::try_from(percent).ok()
    }
}

/// Runs `cargo xtask coverage`.
///
/// # Errors
///
/// [`Error::Check`] in [`Mode::Check`] when a crate's coverage dropped;
/// errors of `cargo llvm-cov` and of reading its report otherwise.
pub fn run(workspace: &Workspace, mode: Mode) -> Result<()> {
    let root = &workspace.root;
    measure(root)?;
    let report = std::fs::read_to_string(root.join(REPORT))
        .map_err(|error| Error::io(format!("reading {REPORT}"), error))?;
    let all = per_crate(&report, workspace, &test_code(workspace)?)?;
    let path = root.join(BASELINE);
    let committed = match std::fs::read_to_string(&path) {
        Ok(text) => baseline::parse(&text)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => baseline::Baseline::new(),
        Err(error) => return Err(Error::io(format!("reading {BASELINE}"), error)),
    };
    let table = table(&all, &committed);
    print!("{table}");
    if process::is_github_actions() {
        process::job_summary(&format!("## Line coverage\n\n```text\n{table}```\n"))?;
    }
    match mode {
        Mode::Report => Ok(()),
        Mode::Bless => {
            let text = baseline::format(&as_baseline(&all), HEADER, &[KEY]);
            std::fs::write(&path, text)
                .map_err(|error| Error::io(format!("writing {BASELINE}"), error))?;
            println!("coverage: wrote {BASELINE}");
            Ok(())
        }
        Mode::Check => check(&all, &committed),
    }
}

/// Runs the tests under `cargo llvm-cov`, checking the recordings of the
/// legacy results, and writes [`REPORT`].
///
/// The runs keep their profiles, so `cargo llvm-cov report` can write other
/// formats afterwards (CI writes `lcov.info` for the coverage artifact).
fn measure(root: &Path) -> Result<()> {
    for program in measure_programs(root) {
        program.run()?;
    }
    Ok(())
}

/// The programs [`measure`] runs.
fn measure_programs(root: &Path) -> Vec<process::Program> {
    vec![
        cargo::cargo(root).args(["llvm-cov", "clean", "--workspace"]),
        cargo::cargo(root).args(["llvm-cov", "--no-report", "--workspace", "--locked"]),
        cargo::cargo(root)
            .args(["llvm-cov", "report", "--lcov"])
            .args(["--output-path", REPORT]),
    ]
}

/// Adds up the lines of each file of a `cargo llvm-cov --lcov` report per
/// crate, leaving out the lines of `tests`. Files outside every crate are
/// left out.
///
/// The report has a record per file: `SF:PATH`, then `DA:LINE,COUNT` for
/// each line with code, and `end_of_record`.
///
/// # Errors
///
/// [`Error::Format`] when a line count is not two numbers, or comes before
/// a file.
pub fn per_crate(
    report: &str,
    workspace: &Workspace,
    tests: &TestCode,
) -> Result<BTreeMap<String, Lines>> {
    let bad = |what: String| Error::Format(format!("{REPORT}: {what}"));
    let mut all: BTreeMap<String, Lines> = BTreeMap::new();
    let mut current: Option<(&str, Option<&TestLines>)> = None;
    let mut in_file = false;
    for line in report.lines() {
        if let Some(path) = line.strip_prefix("SF:") {
            let path = Path::new(path);
            current = package_of(path, workspace).map(|package| (package, tests.get(path)));
            in_file = true;
        } else if line == "end_of_record" {
            current = None;
            in_file = false;
        } else if let Some(data) = line.strip_prefix("DA:") {
            if !in_file {
                return Err(bad(format!("`{line}` before a file")));
            }
            let mut fields = data.split(',');
            let number = fields.next().and_then(|n| n.parse::<usize>().ok());
            let count = fields.next().and_then(|n| n.parse::<u64>().ok());
            let (Some(number), Some(count)) = (number, count) else {
                return Err(bad(format!("`{line}` is not a line count")));
            };
            let Some((package, file_tests)) = current else {
                continue;
            };
            if file_tests.is_some_and(|tests| tests.is_test(number)) {
                continue;
            }
            let total = all.entry(package.to_owned()).or_default();
            total.count += 1;
            total.covered += u64::from(count > 0);
        }
    }
    Ok(all)
}

/// The crate whose directory holds `file`; the innermost one wins, so that
/// `crates/zbm` does not claim `crates/zbm-app` (paths compare by component).
fn package_of<'w>(file: &Path, workspace: &'w Workspace) -> Option<&'w str> {
    workspace
        .packages
        .iter()
        .filter(|package| file.starts_with(&package.dir))
        .max_by_key(|package| package.dir.components().count())
        .map(|package| package.name.as_str())
}

/// The coverage of `all` as a baseline; crates without code are left out.
fn as_baseline(all: &BTreeMap<String, Lines>) -> baseline::Baseline {
    all.iter()
        .filter_map(|(name, lines)| {
            let percent = lines.percent()?;
            Some((name.clone(), [(KEY.to_owned(), percent)].into()))
        })
        .collect()
}

/// The coverage a crate must reach: its baseline, or [`NEW_CRATE`].
fn required(name: &str, committed: &baseline::Baseline) -> usize {
    committed
        .get(name)
        .and_then(|metrics| metrics.get(KEY))
        .copied()
        .unwrap_or(NEW_CRATE)
}

/// Compares `all` with `committed`.
///
/// # Errors
///
/// [`Error::Check`] listing every crate below what it must reach.
pub fn check(all: &BTreeMap<String, Lines>, committed: &baseline::Baseline) -> Result<()> {
    let mut problems = Vec::new();
    let mut improved = false;
    for (name, lines) in all {
        let Some(now) = lines.percent() else {
            continue;
        };
        let needed = required(name, committed);
        if now < needed {
            problems.push(format!("{name}: line coverage {now}% is below {needed}%"));
        }
        improved |= committed.contains_key(name) && now > needed;
    }
    // A crate the report lost would otherwise never be checked again.
    for name in committed.keys() {
        if all.get(name).and_then(|lines| lines.percent()).is_none() {
            problems.push(format!("{name}: has a baseline but no code in the report"));
        }
    }
    if !problems.is_empty() {
        return Err(Error::Check {
            check: "coverage",
            problems,
        });
    }
    if improved {
        println!("coverage: some crates improved; run `cargo xtask coverage --bless` to keep it");
    }
    println!("coverage: no crate dropped");
    Ok(())
}

/// The coverage per crate as text, with what each crate must reach.
pub fn table(all: &BTreeMap<String, Lines>, committed: &baseline::Baseline) -> String {
    let mut text = format!(
        "{:<16} {:>7} {:>7} {:>6} {:>6}\n",
        "crate", "lines", "covered", "now", "needed"
    );
    let (mut count, mut covered) = (0, 0);
    for (name, lines) in all {
        count += lines.count;
        covered += lines.covered;
        let now = lines
            .percent()
            .map_or_else(|| "-".to_owned(), |p| format!("{p}%"));
        let needed = format!("{}%", required(name, committed));
        let _ = writeln!(
            text,
            "{name:<16} {:>7} {:>7} {now:>6} {needed:>6}",
            lines.count, lines.covered
        );
    }
    let total = Lines { count, covered }
        .percent()
        .map_or_else(|| "-".to_owned(), |p| format!("{p}%"));
    let _ = writeln!(text, "{:<16} {count:>7} {covered:>7} {total:>6}", "total");
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Package;
    use std::path::PathBuf;

    fn package(name: &str, dir: &str) -> Package {
        Package {
            name: name.to_owned(),
            dir: PathBuf::from(dir),
            targets: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    fn workspace() -> Workspace {
        Workspace {
            root: PathBuf::from("/w"),
            packages: vec![
                package("zbm", "/w/crates/zbm"),
                package("zbm-app", "/w/crates/zbm-app"),
            ],
        }
    }

    /// The LCOV record of `path`, with a count for each of `counts`, the
    /// first on line 1.
    fn record(path: &str, counts: &[u64]) -> String {
        let mut text = format!("SF:{path}\n");
        for (index, count) in counts.iter().enumerate() {
            let _ = writeln!(text, "DA:{},{count}", index + 1);
        }
        text.push_str("end_of_record\n");
        text
    }

    fn lines(count: u64, covered: u64) -> Lines {
        Lines { count, covered }
    }

    #[test]
    fn files_add_up_per_crate_and_the_innermost_crate_wins() {
        let report = [
            record("/w/crates/zbm/src/main.rs", &[1, 0, 1, 0]),
            record("/w/crates/zbm/src/pilot/menu.rs", &[3, 3]),
            record("/w/crates/zbm-app/src/lib.rs", &[0, 0, 1]),
            record("/elsewhere/lib.rs", &[0; 10]),
        ]
        .concat();

        let all = per_crate(&report, &workspace(), &TestCode::new()).unwrap();

        assert_eq!(all.get("zbm"), Some(&lines(6, 4)));
        assert_eq!(all.get("zbm-app"), Some(&lines(3, 1)));
        assert_eq!(all.len(), 2);
        let none = TestCode::new();
        assert!(per_crate("DA:1,1\n", &workspace(), &none).is_err());
        assert!(per_crate("SF:/w/crates/zbm/a.rs\nDA:x\n", &workspace(), &none).is_err());
    }

    #[test]
    fn test_code_is_left_out() {
        let report = [
            record("/w/crates/zbm/src/main.rs", &[1, 0, 1, 1, 1]),
            record("/w/crates/zbm/src/tests.rs", &[1, 1, 1]),
        ]
        .concat();
        let mut tests = TestCode::new();
        tests.insert(
            PathBuf::from("/w/crates/zbm/src/main.rs"),
            TestLines {
                whole: false,
                ranges: vec![(3, 4)],
            },
        );
        tests.insert(
            PathBuf::from("/w/crates/zbm/src/tests.rs"),
            TestLines {
                whole: true,
                ranges: Vec::new(),
            },
        );

        let all = per_crate(&report, &workspace(), &tests).unwrap();

        // Lines 1, 2 and 5 of main.rs: the test module's lines 3 and 4, and
        // the test file, are left out.
        assert_eq!(all.get("zbm"), Some(&lines(3, 2)));
    }

    #[test]
    fn the_percent_is_rounded_down_and_absent_without_code() {
        assert_eq!(lines(3, 2).percent(), Some(66));
        assert_eq!(lines(0, 0).percent(), None);
    }

    #[test]
    fn a_drop_below_the_baseline_or_a_new_crate_below_80_fails() {
        let committed = baseline::parse("crate line_coverage\nold 60\n").unwrap();
        let mut all = BTreeMap::new();
        all.insert("old".to_owned(), lines(100, 60));
        all.insert("new".to_owned(), lines(100, 80));
        all.insert("empty".to_owned(), lines(0, 0));
        assert!(check(&all, &committed).is_ok());

        all.insert("old".to_owned(), lines(100, 59));
        all.insert("new".to_owned(), lines(100, 79));
        let error = check(&all, &committed).unwrap_err();

        assert_eq!(
            error.to_string(),
            "coverage found 2 problem(s)\n  \
             new: line coverage 79% is below 80%\n  \
             old: line coverage 59% is below 60%"
        );
    }

    #[test]
    fn a_crate_missing_from_the_report_fails() {
        let committed = baseline::parse("crate line_coverage\nold 60\ngone 70\n").unwrap();
        let mut all = BTreeMap::new();
        all.insert("old".to_owned(), lines(100, 60));

        let error = check(&all, &committed).unwrap_err();

        assert_eq!(
            error.to_string(),
            "coverage found 1 problem(s)\n  gone: has a baseline but no code in the report"
        );
    }

    #[test]
    fn the_tests_run_instrumented() {
        let programs = measure_programs(Path::new("/w"));
        let shown: Vec<&str> = programs.iter().map(process::Program::shown).collect();

        assert_eq!(
            shown,
            [
                "cargo llvm-cov clean --workspace",
                "cargo llvm-cov --no-report --workspace --locked",
                "cargo llvm-cov report --lcov --output-path target/coverage.lcov",
            ]
        );
    }

    #[test]
    fn a_blessed_baseline_parses_back() {
        let mut all = BTreeMap::new();
        all.insert("zbm".to_owned(), lines(200, 150));
        all.insert("empty".to_owned(), lines(0, 0));

        let text = baseline::format(&as_baseline(&all), HEADER, &[KEY]);

        let parsed = baseline::parse(&text).unwrap();
        assert_eq!(required("zbm", &parsed), 75);
        assert!(!parsed.contains_key("empty"));
    }
}
