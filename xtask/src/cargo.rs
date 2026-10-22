//! The `cargo` runs that `xtask` is made of: tests, snapshots,
//! documentation, clippy.

use std::path::Path;
use std::process::Stdio;

use crate::error::Result;
use crate::process::Program;
use crate::workspace::Workspace;

/// `cargo` in the workspace root `root`.
pub fn cargo(root: &Path) -> Program {
    Program::new("cargo", root)
}

/// Runs every test that needs no root and no virtual machine: the
/// workspace, then the test build of `zbm` (feature `test-seams`).
///
/// With `cargo-nextest` installed the tests run under `cargo nextest run`
/// (profiles in `.config/nextest.toml`; CI sets `NEXTEST_PROFILE=ci`) and
/// the documentation tests, which nextest cannot run, under `cargo test
/// --doc`. Without it everything runs under `cargo test`.
///
/// # Errors
///
/// When a test fails.
pub fn test(root: &Path) -> Result<()> {
    let runner = if has_nextest(root) {
        ["nextest", "run"].as_slice()
    } else {
        println!("test: cargo-nextest is not installed; using cargo test");
        ["test"].as_slice()
    };
    cargo(root)
        .args(runner)
        .args(["--workspace", "--locked"])
        .run()?;
    if runner != ["test"] {
        cargo(root)
            .args(["test", "--workspace", "--doc", "--locked"])
            .run()?;
    }
    cargo(root)
        .args(runner)
        .args(["-p", "zbm", "--features", "test-seams", "--locked"])
        .run()
}

/// Whether `cargo nextest` runs.
fn has_nextest(root: &Path) -> bool {
    std::process::Command::new("cargo")
        .args(["nextest", "--version"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Accepts every changed snapshot: runs the tests of the crates that use
/// `insta` with `INSTA_UPDATE=always`, which writes each new snapshot over
/// the old one, then lists the snapshot files that changed so that they can
/// be reviewed with `git diff` before they are committed.
///
/// # Errors
///
/// When a test fails for another reason than a snapshot.
pub fn bless(workspace: &Workspace) -> Result<()> {
    let root = &workspace.root;
    let mut args = vec!["test".to_owned(), "--locked".to_owned()];
    for package in snapshot_packages(workspace) {
        args.push("-p".to_owned());
        args.push(package.to_owned());
    }
    cargo(root).args(args).env("INSTA_UPDATE", "always").run()?;
    let changed = Program::new("git", root)
        .args(["status", "--short", "--", "*.snap"])
        .output()?;
    if changed.is_empty() {
        println!("bless: no snapshot changed");
    } else {
        println!("bless: these snapshots changed; review them with `git diff`:");
        print!("{changed}");
    }
    Ok(())
}

/// The crates with snapshot tests: those that depend on `insta`.
fn snapshot_packages(workspace: &Workspace) -> Vec<&str> {
    workspace
        .packages
        .iter()
        .filter(|package| package.dependencies.iter().any(|d| d.name == "insta"))
        .map(|package| package.name.as_str())
        .collect()
}

/// Builds the documentation of the workspace, with warnings as errors.
///
/// # Errors
///
/// When rustdoc warns.
pub fn doc(root: &Path, open: bool) -> Result<()> {
    let mut args = vec!["doc", "--workspace", "--no-deps", "--locked"];
    if open {
        args.push("--open");
    }
    cargo(root)
        .args(args)
        .env("RUSTDOCFLAGS", "-D warnings")
        .run()
}

/// Runs clippy over every target with warnings as errors, then over the
/// test build of `zbm`.
///
/// # Errors
///
/// When clippy warns.
pub fn clippy(root: &Path) -> Result<()> {
    cargo(root)
        .args(["clippy", "--workspace", "--all-targets", "--locked", "--"])
        .args(["-D", "warnings"])
        .run()?;
    cargo(root)
        .args([
            "clippy",
            "-p",
            "zbm",
            "--all-targets",
            "--features",
            "test-seams",
        ])
        .args(["--locked", "--", "-D", "warnings"])
        .run()
}

/// Checks that `rustfmt` would change nothing.
///
/// # Errors
///
/// When a file is not formatted.
pub fn fmt_check(root: &Path) -> Result<()> {
    cargo(root).args(["fmt", "--all", "--check"]).run()
}
