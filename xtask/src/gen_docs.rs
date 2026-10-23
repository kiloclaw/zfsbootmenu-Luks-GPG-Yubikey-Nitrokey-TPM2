//! `cargo xtask gen-docs`: the option sections of the manual pages, written
//! from the table of options in `zbm-config` (`zbm_config::spec`).
//!
//! The work is done by the example `gen_docs` of `zbm-config`, which this
//! runs: developer tools depend on no crate of the workspace, so that they
//! still run when one of those does not build.

use std::path::Path;

use crate::error::Result;
use crate::process::Program;

/// The command line of the example, for `root` and `check`.
fn arguments(root: &Path, check: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "run",
        "--quiet",
        "--locked",
        "-p",
        "zbm-config",
        "--example",
        "gen_docs",
        "--",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(root.display().to_string());
    if check {
        args.push("--check".to_owned());
    }
    args
}

/// Writes the generated sections of every page, or with `check`, fails when
/// one is out of date.
///
/// # Errors
///
/// [`crate::error::Error::Failed`] when the example fails: a page is out of
/// date (with `check`), lacks a marker, or cannot be read or written.
pub fn run(root: &Path, check: bool) -> Result<()> {
    Program::new("cargo", root)
        .args(arguments(root, check))
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_is_run_on_the_repository() {
        let args = arguments(Path::new("/repo"), true);
        assert_eq!(
            args.join(" "),
            "run --quiet --locked -p zbm-config --example gen_docs -- /repo --check"
        );
        assert!(!arguments(Path::new("/repo"), false).contains(&"--check".to_owned()));
    }
}
