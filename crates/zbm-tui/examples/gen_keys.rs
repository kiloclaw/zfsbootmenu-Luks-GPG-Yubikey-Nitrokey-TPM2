//! Writes the key section of `zfsbootmenu(7)` from the key map of the menu
//! (`zbm_tui::menu::keymap::manual`); `cargo xtask gen-docs` runs it after
//! `zbm-config`'s `gen_docs`.
//!
//! Usage: `gen_keys <repository> [--check]`. With `--check`, it writes
//! nothing and fails when the page is out of date.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_macros,
    reason = "a developer tool: it reads and writes the page and reports on the terminal"
)]

use std::path::Path;
use std::process::ExitCode;

use zbm_config::docs;
use zbm_tui::menu::keymap::manual;

/// The page, relative to the top of the repository.
const PAGE: &str = "docs/man/zfsbootmenu.7.rst";

/// Updates or checks the page below `root`; returns the problem, if any.
fn run(root: &Path, check: bool) -> Result<(), String> {
    let path = root.join(PAGE);
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("reading {PAGE}: {error}"))?;
    let updated = docs::splice(PAGE, &text, "keys", &manual::workspace())
        .and_then(|text| docs::splice(PAGE, &text, "keys-aliases", &manual::aliases()))
        .map_err(|error| error.to_string())?;
    if updated == text {
        return Ok(());
    }
    if check {
        return Err(format!("{PAGE} is out of date: run `cargo xtask gen-docs`"));
    }
    std::fs::write(&path, updated).map_err(|error| format!("writing {PAGE}: {error}"))?;
    println!("gen-keys: wrote {PAGE}");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (root, check) = match args.as_slice() {
        [root] => (root, false),
        [root, flag] if flag == "--check" => (root, true),
        _ => {
            eprintln!("Usage: gen_keys <repository> [--check]");
            return ExitCode::from(2);
        }
    };
    match run(Path::new(root), check) {
        Ok(()) => {
            println!("gen-keys: the key section of {PAGE} is up to date");
            ExitCode::SUCCESS
        }
        Err(problem) => {
            eprintln!("gen-keys: {problem}");
            ExitCode::FAILURE
        }
    }
}
