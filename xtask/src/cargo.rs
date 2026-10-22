//! The `cargo` runs that `xtask` is made of: tests, documentation, clippy.

use std::path::Path;

use crate::error::Result;
use crate::process::Program;

/// `cargo` in the workspace root `root`.
pub fn cargo(root: &Path) -> Program {
    Program::new("cargo", root)
}

/// Runs every test that needs no root and no virtual machine: the
/// workspace, then the test build of `zbm` (feature `test-seams`).
///
/// # Errors
///
/// When a test fails.
pub fn test(root: &Path) -> Result<()> {
    cargo(root)
        .args(["test", "--workspace", "--locked"])
        .run()?;
    cargo(root)
        .args(["test", "-p", "zbm", "--features", "test-seams", "--locked"])
        .run()
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
