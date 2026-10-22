//! `cargo xtask lint`: every check that a pull request must pass, and the
//! report of the stricter lints that refactored code will have to pass.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;

use crate::cargo;
use crate::error::{Error, Result};
use crate::metrics::{self, Mode};
use crate::process;
use crate::workspace::Workspace;
use crate::{deps, widths};

/// The directories whose line widths are checked.
pub const WIDTH_ROOTS: &[&str] = &["crates", "xtask"];

/// The lints that crates must pass once a phase of REFACTOR-ROADMAP.md has
/// refactored them, on top of the workspace lints in `Cargo.toml`.
pub const STRICT_LINTS: &[&str] = &[
    "clippy::pedantic",
    "clippy::unwrap_used",
    "clippy::expect_used",
    "clippy::panic",
    "clippy::indexing_slicing",
    "clippy::unreachable",
    "clippy::todo",
    "clippy::dbg_macro",
];

/// Lints of [`STRICT_LINTS`] that refactored code does not follow, each with
/// the reason.
///
/// - `clippy::must_use_candidate` asks for `#[must_use]` on every getter. The
///   refactoring puts it where ignoring a value is a mistake (outcomes and
///   errors), as the Rust API Guidelines do, not on every method.
pub const STRICT_ALLOWED: &[&str] = &["clippy::must_use_candidate"];

/// The directory of the strict `clippy.toml`, relative to the root.
pub const STRICT_CONFIG: &str = "xtask/strict";

/// Runs every check and reports all failures together.
///
/// # Errors
///
/// [`Error::Check`] naming each check that failed.
pub fn run(workspace: &Workspace) -> Result<()> {
    let root = &workspace.root;
    let mut failed = Vec::new();
    step(&mut failed, "format", || cargo::fmt_check(root));
    step(&mut failed, "line widths", || check_widths(root));
    step(&mut failed, "clippy", || cargo::clippy(root));
    step(&mut failed, "documentation", || cargo::doc(root, false));
    step(&mut failed, "check-deps", || deps::run(workspace, false));
    step(&mut failed, "metrics", || {
        metrics::run(workspace, Mode::Check)
    });
    if failed.is_empty() {
        println!("lint: every check passed");
        Ok(())
    } else {
        Err(Error::Check {
            check: "lint",
            problems: failed,
        })
    }
}

/// Runs one check, printing and recording its failure.
fn step(failed: &mut Vec<String>, name: &str, check: impl FnOnce() -> Result<()>) {
    println!("\n== {name}");
    if let Err(error) = check() {
        eprintln!("{name}: {error}");
        failed.push(format!("{name}: {error}"));
    }
}

/// Checks the line widths below [`WIDTH_ROOTS`].
///
/// # Errors
///
/// [`Error::Check`] listing every line that is too long.
pub fn check_widths(root: &Path) -> Result<()> {
    let problems = widths::check(root, WIDTH_ROOTS)?;
    if problems.is_empty() {
        println!("line widths: every line fits");
        Ok(())
    } else {
        Err(Error::Check {
            check: "line widths",
            problems,
        })
    }
}

/// Reports the [`STRICT_LINTS`] and the disallowed methods of
/// `xtask/strict/clippy.toml`, per crate and per lint.
///
/// This never fails on findings: it shows how much work the refactor has
/// left. It builds in `target/strict` so that it does not invalidate the
/// normal build.
///
/// # Errors
///
/// When clippy cannot run.
pub fn strict(workspace: &Workspace) -> Result<()> {
    let root = &workspace.root;
    let config = root.join(STRICT_CONFIG);
    let target = root.join("target/strict");
    let mut args = vec![
        "clippy".to_owned(),
        "--workspace".to_owned(),
        "--all-targets".to_owned(),
        "--locked".to_owned(),
        "--message-format=json".to_owned(),
        "--target-dir".to_owned(),
        target.display().to_string(),
        "--".to_owned(),
    ];
    for lint in STRICT_LINTS {
        args.push("-W".to_owned());
        args.push((*lint).to_owned());
    }
    for lint in STRICT_ALLOWED {
        args.push("-A".to_owned());
        args.push((*lint).to_owned());
    }
    let output = cargo::cargo(root)
        .env("CLIPPY_CONF_DIR", &config.display().to_string())
        .args(args)
        .output()?;
    let findings = findings(&output, workspace);
    let report = report(&findings);
    print!("{report}");
    std::fs::write(target.join("strict-lints.txt"), &report)
        .map_err(|error| Error::io("writing target/strict/strict-lints.txt", error))?;
    if process::is_github_actions() {
        process::job_summary(&format!("## Strict lints\n\n```text\n{report}```\n"))?;
    }
    Ok(())
}

/// Counts clippy's warnings per (crate, lint) from its JSON messages.
///
/// A warning in a library shows up twice, once for the library and once for
/// its unit tests; each place is counted once.
pub fn findings(json: &str, workspace: &Workspace) -> BTreeMap<(String, String), usize> {
    let places: BTreeSet<_> = json
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| finding(&value, workspace))
        .collect();
    let mut counts = BTreeMap::new();
    for (package, lint, _) in places {
        *counts.entry((package, lint)).or_insert(0) += 1;
    }
    counts
}

/// The place in the source a clippy finding is about.
type Place = Option<(String, u64, u64)>;

/// The package, lint and place of one JSON message, if it is a lint
/// finding in a workspace package.
fn finding(value: &Value, workspace: &Workspace) -> Option<(String, String, Place)> {
    if value.get("reason")?.as_str()? != "compiler-message" {
        return None;
    }
    let message = value.get("message")?;
    let lint = message.get("code")?.get("code")?.as_str()?;
    let package = package_of(value, workspace)?;
    let primary = message
        .get("spans")
        .and_then(Value::as_array)
        .and_then(|spans| {
            spans
                .iter()
                .find(|span| span.get("is_primary") == Some(&Value::Bool(true)))
        });
    let place = primary.map(|span| {
        let number = |key: &str| span.get(key).and_then(Value::as_u64).unwrap_or_default();
        let file = span
            .get("file_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        (
            file.to_owned(),
            number("line_start"),
            number("column_start"),
        )
    });
    Some((package, lint.to_owned(), place))
}

/// The name of the workspace package a JSON message is about.
fn package_of(value: &Value, workspace: &Workspace) -> Option<String> {
    let manifest = Path::new(value.get("manifest_path")?.as_str()?);
    let dir = manifest.parent()?;
    workspace
        .packages
        .iter()
        .find(|package| package.dir == dir)
        .map(|package| package.name.clone())
}

/// The findings as text: totals per crate, then per lint.
pub fn report(findings: &BTreeMap<(String, String), usize>) -> String {
    let mut per_crate: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_lint: BTreeMap<&str, usize> = BTreeMap::new();
    for ((package, lint), count) in findings {
        *per_crate.entry(package).or_default() += count;
        *per_lint.entry(lint).or_default() += count;
    }
    let total: usize = per_crate.values().sum();
    let mut text = format!("strict lints: {total} findings\n\nper crate:\n");
    for (package, count) in &per_crate {
        let _ = writeln!(text, "  {package:<16} {count:>6}");
    }
    let mut lints: Vec<(&str, usize)> = per_lint.into_iter().collect();
    lints.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    text.push_str("\nper lint:\n");
    for (lint, count) in lints {
        let _ = writeln!(text, "  {lint:<44} {count:>6}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Package;
    use std::path::PathBuf;

    fn workspace() -> Workspace {
        Workspace {
            root: PathBuf::from("/w"),
            packages: vec![Package {
                name: "zbm-core".to_owned(),
                dir: PathBuf::from("/w/crates/zbm-core"),
                targets: Vec::new(),
                dependencies: Vec::new(),
            }],
        }
    }

    fn message(lint: &str, line_start: u64) -> String {
        serde_json::json!({
            "reason": "compiler-message",
            "manifest_path": "/w/crates/zbm-core/Cargo.toml",
            "message": {
                "code": { "code": lint },
                "spans": [{
                    "is_primary": true,
                    "file_name": "src/lib.rs",
                    "line_start": line_start,
                    "column_start": 1
                }]
            }
        })
        .to_string()
    }

    #[test]
    fn the_same_place_counts_once_even_when_reported_twice() {
        let json = [
            message("clippy::unwrap_used", 3),
            message("clippy::unwrap_used", 3),
            message("clippy::unwrap_used", 9),
            "{\"reason\":\"build-finished\"}".to_owned(),
        ]
        .join("\n");

        let findings = findings(&json, &workspace());

        let key = ("zbm-core".to_owned(), "clippy::unwrap_used".to_owned());
        assert_eq!(findings.get(&key), Some(&2));
    }

    #[test]
    fn the_report_lists_the_most_frequent_lint_first() {
        let mut findings = BTreeMap::new();
        findings.insert(("a".to_owned(), "clippy::panic".to_owned()), 1);
        findings.insert(("a".to_owned(), "clippy::todo".to_owned()), 4);

        let text = report(&findings);

        assert!(text.starts_with("strict lints: 5 findings\n"));
        let todo = text.find("clippy::todo").unwrap();
        let panic = text.find("clippy::panic").unwrap();
        assert!(todo < panic);
    }
}
