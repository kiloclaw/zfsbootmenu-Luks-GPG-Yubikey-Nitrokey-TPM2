//! Writes the option sections of the manual pages from the table of options;
//! `cargo xtask gen-docs` runs it.
//!
//! Usage: `gen_docs <repository> [--check]`. With `--check`, it writes
//! nothing and fails when a page is out of date.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_macros,
    reason = "a developer tool: it reads and writes the pages and reports on the terminal"
)]

use std::path::Path;
use std::process::ExitCode;

use zbm_config::docs;

/// Updates or checks every page below `root`; returns the problems.
fn run(root: &Path, check: bool) -> Vec<String> {
    let mut problems = Vec::new();
    for page in docs::pages() {
        let path = root.join(page);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                problems.push(format!("reading {page}: {error}"));
                continue;
            }
        };
        let updated = match docs::update(page, &text) {
            Ok(updated) => updated,
            Err(error) => {
                problems.push(error.to_string());
                continue;
            }
        };
        if updated == text {
            continue;
        }
        if check {
            problems.push(format!("{page} is out of date: run `cargo xtask gen-docs`"));
        } else if let Err(error) = std::fs::write(&path, updated) {
            problems.push(format!("writing {page}: {error}"));
        } else {
            println!("gen-docs: wrote {page}");
        }
    }
    problems
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (root, check) = match args.as_slice() {
        [root] => (root, false),
        [root, flag] if flag == "--check" => (root, true),
        _ => {
            eprintln!("Usage: gen_docs <repository> [--check]");
            return ExitCode::from(2);
        }
    };
    let problems = run(Path::new(root), check);
    if problems.is_empty() {
        println!("gen-docs: the option sections of the manual pages are up to date");
        return ExitCode::SUCCESS;
    }
    for problem in &problems {
        eprintln!("gen-docs: {problem}");
    }
    ExitCode::FAILURE
}
