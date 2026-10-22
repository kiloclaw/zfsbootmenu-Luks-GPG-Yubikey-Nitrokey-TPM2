//! `cargo xtask coverage`: line coverage per crate, which may only go up.
//!
//! The tests run under `cargo llvm-cov` (the workspace, then the test build
//! of `zbm`), and the lines of each source file count for the crate whose
//! directory holds it. `--check` compares the whole percent of each crate
//! with the committed baseline (`xtask/coverage-baseline.txt`) and fails
//! when it dropped; a crate that is not in the baseline must reach
//! [`NEW_CRATE`]. `--bless` writes the current numbers as the new baseline.
//!
//! This is a command of its own, not part of `metrics`, because it runs
//! every test in an instrumented build. The tests run with `ZBM_PARITY=check`,
//! so the parity tests also run the legacy code they were recorded from and
//! fail when a recording is out of date (`zbm_testkit::parity`); that needs
//! what `cargo xtask record-parity` needs.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;

use crate::cargo;
use crate::error::{Error, Result};
use crate::metrics::{Mode, baseline};
use crate::process;
use crate::workspace::Workspace;

/// The baseline file, relative to the workspace root.
pub const BASELINE: &str = "xtask/coverage-baseline.txt";

/// The line coverage, in percent, that a crate missing from the baseline
/// must reach.
pub const NEW_CRATE: usize = 80;

/// The column of the baseline.
const KEY: &str = "line_coverage";

/// The report of `cargo llvm-cov`, relative to the workspace root.
const REPORT: &str = "target/coverage.json";

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
    let json = std::fs::read_to_string(root.join(REPORT))
        .map_err(|error| Error::io(format!("reading {REPORT}"), error))?;
    let all = per_crate(&json, workspace)?;
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
    cargo::cargo(root)
        .args(["llvm-cov", "clean", "--workspace"])
        .run()?;
    cargo::cargo(root)
        .env("ZBM_PARITY", "check")
        .args(["llvm-cov", "--no-report", "--workspace", "--locked"])
        .run()?;
    cargo::cargo(root)
        .env("ZBM_PARITY", "check")
        .args(["llvm-cov", "--no-report", "-p", "zbm"])
        .args(["--features", "test-seams", "--locked"])
        .run()?;
    cargo::cargo(root)
        .args(["llvm-cov", "report", "--json", "--summary-only"])
        .args(["--output-path", REPORT])
        .run()
}

/// Adds up the lines of each file of a `cargo llvm-cov --json` report per
/// crate. Files outside every crate are left out.
///
/// # Errors
///
/// [`Error::Format`] when the report is not the JSON export of LLVM.
pub fn per_crate(json: &str, workspace: &Workspace) -> Result<BTreeMap<String, Lines>> {
    let bad = |what: &str| Error::Format(format!("{REPORT}: {what}"));
    let report: Value = serde_json::from_str(json).map_err(|_| bad("not JSON"))?;
    let files = report
        .pointer("/data/0/files")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("no data[0].files"))?;
    let mut all = BTreeMap::new();
    for file in files {
        let name = file
            .get("filename")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("a file without a name"))?;
        let Some(package) = package_of(Path::new(name), workspace) else {
            continue;
        };
        let lines = file.pointer("/summary/lines");
        let number = |key: &str| lines.and_then(|l| l.get(key)).and_then(Value::as_u64);
        let (Some(count), Some(covered)) = (number("count"), number("covered")) else {
            return Err(bad(&format!("{name} has no line summary")));
        };
        let total: &mut Lines = all.entry(package.to_owned()).or_default();
        total.count += count;
        total.covered += covered;
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

    fn file(name: &str, count: u64, covered: u64) -> Value {
        serde_json::json!({
            "filename": name,
            "summary": { "lines": { "count": count, "covered": covered } }
        })
    }

    fn lines(count: u64, covered: u64) -> Lines {
        Lines { count, covered }
    }

    #[test]
    fn files_add_up_per_crate_and_the_innermost_crate_wins() {
        let report = serde_json::json!({
            "data": [{ "files": [
                file("/w/crates/zbm/src/main.rs", 10, 5),
                file("/w/crates/zbm/src/pilot/menu.rs", 10, 10),
                file("/w/crates/zbm-app/src/lib.rs", 4, 1),
                file("/elsewhere/lib.rs", 100, 0),
            ]}]
        });

        let all = per_crate(&report.to_string(), &workspace()).unwrap();

        assert_eq!(all.get("zbm"), Some(&lines(20, 15)));
        assert_eq!(all.get("zbm-app"), Some(&lines(4, 1)));
        assert_eq!(all.len(), 2);
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
