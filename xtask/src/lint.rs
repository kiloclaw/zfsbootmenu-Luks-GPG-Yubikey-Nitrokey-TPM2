//! `cargo xtask lint`: every check that a pull request must pass, and the
//! report of the stricter lints that refactored code will have to pass.
//!
//! The crates listed in `xtask/strict/crates.txt` have reached the strict
//! lints and must stay there: `lint` runs the strict lints over them and
//! fails on any finding, and so does `lint --strict`, which also reports
//! every other crate.

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
pub const WIDTH_ROOTS: &[&str] = &["crates", "xtask", "fuzz/fuzz_targets"];

/// The lints that crates must pass once a phase of ROADMAP.md has
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

/// The crates that must have no strict finding, relative to the root.
pub const STRICT_CRATES: &str = "xtask/strict/crates.txt";

/// Findings per (crate, lint).
pub type Findings = BTreeMap<(String, String), usize>;

/// Runs every check and reports all failures together.
///
/// # Errors
///
/// [`Error::Check`] naming each check that failed.
pub fn run(workspace: &Workspace) -> Result<()> {
    let root = &workspace.root;
    let mut failed = Vec::new();
    step(&mut failed, "format", || cargo::fmt_check(root));
    step(&mut failed, "fuzz targets", || crate::fuzz::check(root));
    step(&mut failed, "line widths", || check_widths(root));
    step(&mut failed, "clippy", || cargo::clippy(root));
    step(&mut failed, "strict crates", || strict_crates(workspace));
    step(&mut failed, "documentation", || cargo::doc(root, false));
    step(&mut failed, "gen-docs", || crate::gen_docs::run(root, true));
    step(&mut failed, "check-deps", || deps::run(workspace, false));
    step(&mut failed, "test features", || crate::release::check(root));
    step(&mut failed, "menu text", || crate::texts::check(workspace));
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

/// Runs the strict lints over the crates of [`STRICT_CRATES`] alone.
///
/// # Errors
///
/// [`Error::Check`] naming every finding in those crates, or a name in the
/// list that is not a crate of the workspace.
pub fn strict_crates(workspace: &Workspace) -> Result<()> {
    let listed = listed_crates(workspace)?;
    let mut selection = Vec::new();
    for name in &listed {
        selection.push("-p".to_owned());
        selection.push(name.clone());
    }
    let findings = strict_clippy(workspace, &selection)?;
    check_listed(&findings, &listed)?;
    println!("strict crates: {} crates without a finding", listed.len());
    Ok(())
}

/// Reports the [`STRICT_LINTS`] and the disallowed methods of
/// `xtask/strict/clippy.toml`, per crate and per lint.
///
/// The report shows how much work the refactor has left; it fails only when
/// a crate of [`STRICT_CRATES`] has a finding. It builds in `target/strict`
/// so that it does not invalidate the normal build.
///
/// # Errors
///
/// When clippy cannot run, and as [`strict_crates`].
pub fn strict(workspace: &Workspace) -> Result<()> {
    let listed = listed_crates(workspace)?;
    let findings = strict_clippy(workspace, &["--workspace".to_owned()])?;
    let report = report(&findings);
    print!("{report}");
    let path = workspace.root.join("target/strict/strict-lints.txt");
    std::fs::write(path, &report)
        .map_err(|error| Error::io("writing target/strict/strict-lints.txt", error))?;
    if process::is_github_actions() {
        process::job_summary(&format!("## Strict lints\n\n```text\n{report}```\n"))?;
    }
    check_listed(&findings, &listed)
}

/// Runs clippy with the strict lints over `selection` (`--workspace` or
/// `-p` options) and counts its findings.
fn strict_clippy(workspace: &Workspace, selection: &[String]) -> Result<Findings> {
    let root = &workspace.root;
    let config = root.join(STRICT_CONFIG);
    let args = strict_args(selection, &root.join("target/strict"));
    let output = cargo::cargo(root)
        .env("CLIPPY_CONF_DIR", &config.display().to_string())
        .args(args)
        .output()?;
    Ok(findings(&output, workspace))
}

/// The arguments of the strict clippy run over `selection`, building in
/// `target`.
fn strict_args(selection: &[String], target: &Path) -> Vec<String> {
    let mut args = vec!["clippy".to_owned()];
    args.extend_from_slice(selection);
    args.extend(
        [
            "--all-targets",
            "--locked",
            "--message-format=json",
            "--target-dir",
        ]
        .map(str::to_owned),
    );
    args.push(target.display().to_string());
    args.push("--".to_owned());
    for lint in STRICT_LINTS {
        args.push("-W".to_owned());
        args.push((*lint).to_owned());
    }
    for lint in STRICT_ALLOWED {
        args.push("-A".to_owned());
        args.push((*lint).to_owned());
    }
    args
}

/// The crates of [`STRICT_CRATES`], checked against the workspace.
///
/// # Errors
///
/// [`Error::Check`] for a name that is not a crate of the workspace.
fn listed_crates(workspace: &Workspace) -> Result<Vec<String>> {
    let path = workspace.root.join(STRICT_CRATES);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| Error::io(format!("reading {STRICT_CRATES}"), error))?;
    let listed = parse_crates(&text);
    let unknown: Vec<String> = listed
        .iter()
        .filter(|name| !workspace.packages.iter().any(|p| &p.name == *name))
        .map(|name| format!("{STRICT_CRATES} names {name}, which is not a crate"))
        .collect();
    if unknown.is_empty() {
        Ok(listed)
    } else {
        Err(Error::Check {
            check: "strict crates",
            problems: unknown,
        })
    }
}

/// The crate names of a crate list: one per line, `#` starts a comment.
pub fn parse_crates(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Fails when a crate of `listed` has a finding.
///
/// # Errors
///
/// [`Error::Check`] with one line per crate and lint.
pub fn check_listed(findings: &Findings, listed: &[String]) -> Result<()> {
    let problems: Vec<String> = findings
        .iter()
        .filter(|((package, _), _)| listed.contains(package))
        .map(|((package, lint), count)| format!("{package}: {count} x {lint}"))
        .collect();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(Error::Check {
            check: "strict crates",
            problems,
        })
    }
}

/// Counts clippy's warnings per (crate, lint) from its JSON messages.
///
/// A warning in a library shows up twice, once for the library and once for
/// its unit tests; each place is counted once.
pub fn findings(json: &str, workspace: &Workspace) -> Findings {
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
pub fn report(findings: &Findings) -> String {
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
                name: "zbm-trace".to_owned(),
                dir: PathBuf::from("/w/crates/zbm-trace"),
                targets: Vec::new(),
                dependencies: Vec::new(),
            }],
        }
    }

    fn message(lint: &str, line_start: u64) -> String {
        serde_json::json!({
            "reason": "compiler-message",
            "manifest_path": "/w/crates/zbm-trace/Cargo.toml",
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

        let key = ("zbm-trace".to_owned(), "clippy::unwrap_used".to_owned());
        assert_eq!(findings.get(&key), Some(&2));
    }

    #[test]
    fn the_strict_run_warns_on_the_strict_lints_in_its_own_target() {
        let args = strict_args(&["-p".to_owned(), "xtask".to_owned()], Path::new("/t"));

        assert_eq!(
            args[..8],
            [
                "clippy",
                "-p",
                "xtask",
                "--all-targets",
                "--locked",
                "--message-format=json",
                "--target-dir",
                "/t"
            ]
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-W", "clippy::pedantic"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-A", "clippy::must_use_candidate"])
        );
    }

    #[test]
    fn a_crate_list_has_one_name_per_line_and_comments() {
        let text = "# Crates without a finding\nxtask\n\nzbm-types  # R0\n";

        assert_eq!(parse_crates(text), ["xtask", "zbm-types"]);
    }

    #[test]
    fn only_findings_in_listed_crates_fail() {
        let mut findings = Findings::new();
        findings.insert(("old".to_owned(), "clippy::panic".to_owned()), 3);
        assert!(check_listed(&findings, &["new".to_owned()]).is_ok());

        findings.insert(("new".to_owned(), "clippy::todo".to_owned()), 1);
        let error = check_listed(&findings, &["new".to_owned()]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "strict crates found 1 problem(s)\n  new: 1 x clippy::todo"
        );
    }

    #[test]
    fn the_listed_crates_are_crates_of_the_workspace() {
        let root = crate::workspace_root();
        let text = std::fs::read_to_string(root.join(STRICT_CRATES)).unwrap();
        let workspace = Workspace::load(&root).unwrap();

        for name in parse_crates(&text) {
            assert!(
                workspace.packages.iter().any(|p| p.name == name),
                "{name} is not a crate"
            );
        }
    }

    #[test]
    fn the_report_lists_the_most_frequent_lint_first() {
        let mut findings = Findings::new();
        findings.insert(("a".to_owned(), "clippy::panic".to_owned()), 1);
        findings.insert(("a".to_owned(), "clippy::todo".to_owned()), 4);

        let text = report(&findings);

        assert!(text.starts_with("strict lints: 5 findings\n"));
        let todo = text.find("clippy::todo").unwrap();
        let panic = text.find("clippy::panic").unwrap();
        assert!(todo < panic);
    }
}
