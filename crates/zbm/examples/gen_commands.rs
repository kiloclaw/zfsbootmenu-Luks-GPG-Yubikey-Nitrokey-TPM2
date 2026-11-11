//! Writes the recovery shell's commands into `zfsbootmenu(7)`, its section
//! "The Recovery Shell", from `zbm`'s table of applets and the shell
//! library's functions (`zbm::shell_commands`); `cargo xtask gen-docs` runs
//! it after `zbm-tui`'s `gen_keys`.
//!
//! Usage: `gen_commands <repository> [--check]`. With `--check`, it writes
//! nothing and fails when the page is out of date.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_macros,
    reason = "a developer tool: it reads and writes the page and reports on the terminal"
)]

use std::path::Path;
use std::process::ExitCode;

use zbm_config::docs;

/// The page, relative to the top of the repository.
const PAGE: &str = "docs/man/zfsbootmenu.7.rst";

/// Updates or checks the page below `root`; returns the problem, if any.
/// The crate's test `gen_commands` runs it on a scratch copy of the page.
pub(crate) fn run(root: &Path, check: bool) -> Result<(), String> {
    let path = root.join(PAGE);
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("reading {PAGE}: {error}"))?;
    let updated = docs::splice(PAGE, &text, "shell-commands", &zbm::shell_commands())
        .map_err(|error| error.to_string())?;
    if updated == text {
        return Ok(());
    }
    if check {
        return Err(format!("{PAGE} is out of date: run `cargo xtask gen-docs`"));
    }
    std::fs::write(&path, updated).map_err(|error| format!("writing {PAGE}: {error}"))?;
    println!("gen-commands: wrote {PAGE}");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (root, check) = match args.as_slice() {
        [root] => (root, false),
        [root, flag] if flag == "--check" => (root, true),
        _ => {
            eprintln!("Usage: gen_commands <repository> [--check]");
            return ExitCode::from(2);
        }
    };
    match run(Path::new(root), check) {
        Ok(()) => {
            println!("gen-commands: the recovery shell's commands in {PAGE} are up to date");
            ExitCode::SUCCESS
        }
        Err(problem) => {
            eprintln!("gen-commands: {problem}");
            ExitCode::FAILURE
        }
    }
}
