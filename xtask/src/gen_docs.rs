//! `cargo xtask gen-docs`: the option sections of the manual pages, written
//! from the table of options in `zbm-config` (`zbm_config::spec`), the key
//! section of zfsbootmenu(7), written from the key map in `zbm-tui`
//! (`zbm_tui::menu::keymap::manual`), and the recovery shell's commands in
//! zfsbootmenu(7), written from the table of applets in `zbm`
//! (`zbm::shell_commands`).
//!
//! The work is done by the examples `gen_docs` of `zbm-config`, `gen_keys`
//! of `zbm-tui` and `gen_commands` of `zbm`, which this runs in that
//! order: developer tools depend on no crate of the workspace, so that they
//! still run when one of those does not build.

use std::path::Path;

use crate::error::Result;
use crate::process::Program;

/// The examples that write the generated sections: (package, example).
const EXAMPLES: [(&str, &str); 3] = [
    ("zbm-config", "gen_docs"),
    ("zbm-tui", "gen_keys"),
    ("zbm", "gen_commands"),
];

/// The command line of the example `example` of `package`, for `root` and
/// `check`.
fn arguments(root: &Path, package: &str, example: &str, check: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "run",
        "--quiet",
        "--locked",
        "-p",
        package,
        "--example",
        example,
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
/// [`crate::error::Error::Failed`] when an example fails: a page is out of
/// date (with `check`), lacks a marker, or cannot be read or written.
pub fn run(root: &Path, check: bool) -> Result<()> {
    each(&EXAMPLES, |package, example| {
        Program::new("cargo", root)
            .args(arguments(root, package, example, check))
            .run()
    })
}

/// Runs every one of `examples` with `run`, even after one fails, so that
/// each reports its problems; then the first failure, if any.
fn each(examples: &[(&str, &str)], mut run: impl FnMut(&str, &str) -> Result<()>) -> Result<()> {
    let mut failed = None;
    for (package, example) in examples {
        if let Err(error) = run(package, example) {
            failed.get_or_insert(error);
        }
    }
    failed.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_examples_are_run_on_the_repository() {
        let lines: Vec<String> = EXAMPLES
            .iter()
            .map(|(package, example)| {
                arguments(Path::new("/repo"), package, example, true).join(" ")
            })
            .collect();
        assert_eq!(
            lines,
            [
                "run --quiet --locked -p zbm-config --example gen_docs -- /repo --check",
                "run --quiet --locked -p zbm-tui --example gen_keys -- /repo --check",
                "run --quiet --locked -p zbm --example gen_commands -- /repo --check",
            ]
        );
        let written = arguments(Path::new("/repo"), "zbm-tui", "gen_keys", false);
        assert!(!written.contains(&"--check".to_owned()));
    }

    #[test]
    fn every_example_runs_after_one_fails_and_the_first_failure_is_kept() {
        let mut ran = Vec::new();
        let result = each(&EXAMPLES, |package, example| {
            ran.push(format!("{package} {example}"));
            Err(crate::error::Error::Failed {
                command: example.to_owned(),
            })
        });
        assert_eq!(
            ran,
            [
                "zbm-config gen_docs",
                "zbm-tui gen_keys",
                "zbm gen_commands"
            ]
        );
        assert!(matches!(
            result,
            Err(crate::error::Error::Failed { command }) if command == "gen_docs"
        ));
        assert!(each(&EXAMPLES, |_, _| Ok(())).is_ok());
    }
}
