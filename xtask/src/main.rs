//! Developer automation for the ZFSBootMenu workspace.
//!
//! Run it as `cargo xtask <command>` anywhere in the workspace; the alias is
//! in `.cargo/config.toml`. Each command prints the programs it runs, so a
//! failing step can be repeated by hand. `cargo xtask help` lists the
//! commands. REFACTOR-ROADMAP.md explains why each check exists; the
//! developer documentation is in `docs/dev/`.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_macros,
    reason = "xtask is developer tooling: it runs programs, reads files and prints reports"
)]

mod cargo;
mod coverage;
mod deps;
mod error;
mod fuzz;
mod lint;
mod metrics;
mod process;
mod widths;
mod workspace;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use error::{Error, Result};
use metrics::Mode;
use workspace::Workspace;

/// The text of `cargo xtask help`.
const USAGE: &str = "\
Usage: cargo xtask <command> [options]

Commands:
  lint                   Every check a pull request must pass: format, line
                         widths, clippy, the strict lints of the crates in
                         xtask/strict/crates.txt, documentation, check-deps
                         and metrics --check
  lint --strict          Report the stricter lints that refactored crates
                         must pass, for every crate; fails only on a finding
                         in a crate of xtask/strict/crates.txt
  test                   Every test that needs no root and no virtual
                         machine, under cargo nextest when it is installed
  bless                  Accept every changed snapshot and list the files
  record-parity [--check]
                         Run the legacy code of the parity tests and record
                         its results in tests/golden/legacy; --check fails
                         where a recording no longer matches
  doc [--open]           Build the documentation, warnings as errors
  check-deps [--bless]   Check the dependencies between crates against the
                         layers in xtask/src/deps.rs; --bless rewrites the
                         generated section of docs/dev/architecture.md
  metrics [--check | --bless]
                         Print code metrics per crate; --check fails when a
                         debt metric rose above xtask/metrics-baseline.txt,
                         --bless writes the current numbers there
  fuzz [<target>] [--seconds <n>]
                         Run the fuzz targets of fuzz/ (nightly Rust and
                         cargo-fuzz), each for n seconds (60)
  coverage [--check | --bless]
                         Print line coverage per crate (cargo llvm-cov);
                         --check fails when a crate dropped below
                         xtask/coverage-baseline.txt (new crates: 80%),
                         --bless writes the current numbers there
  widths                 Check the line widths of The Rust Style Guide
  help                   Show this text
";

/// One command of the command line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Command {
    /// `lint`, strict or not.
    Lint { strict: bool },
    /// `test`.
    Test,
    /// `bless`.
    Bless,
    /// `record-parity`.
    RecordParity { check: bool },
    /// `doc`.
    Doc { open: bool },
    /// `check-deps`.
    CheckDeps { bless: bool },
    /// `metrics`.
    Metrics(Mode),
    /// `coverage`.
    Coverage(Mode),
    /// `fuzz`: one target or all, and the seconds each runs.
    Fuzz {
        target: Option<&'static str>,
        seconds: u64,
    },
    /// `widths`.
    Widths,
    /// `help`.
    Help,
}

impl Command {
    /// Parses the arguments after `cargo xtask`.
    fn parse(args: &[String]) -> Result<Command> {
        let words: Vec<&str> = args.iter().map(String::as_str).collect();
        let command = match words.as_slice() {
            [] | ["help" | "--help" | "-h"] => Command::Help,
            ["lint"] => Command::Lint { strict: false },
            ["lint", "--strict"] => Command::Lint { strict: true },
            ["test"] => Command::Test,
            ["bless"] => Command::Bless,
            ["record-parity"] => Command::RecordParity { check: false },
            ["record-parity", "--check"] => Command::RecordParity { check: true },
            ["doc"] => Command::Doc { open: false },
            ["doc", "--open"] => Command::Doc { open: true },
            ["check-deps"] => Command::CheckDeps { bless: false },
            ["check-deps", "--bless"] => Command::CheckDeps { bless: true },
            ["metrics"] => Command::Metrics(Mode::Report),
            ["metrics", "--check"] => Command::Metrics(Mode::Check),
            ["metrics", "--bless"] => Command::Metrics(Mode::Bless),
            ["fuzz", rest @ ..] => parse_fuzz(rest)?,
            ["coverage"] => Command::Coverage(Mode::Report),
            ["coverage", "--check"] => Command::Coverage(Mode::Check),
            ["coverage", "--bless"] => Command::Coverage(Mode::Bless),
            ["widths"] => Command::Widths,
            _ => {
                return Err(Error::Usage(format!(
                    "unknown command `{}`; run `cargo xtask help`",
                    words.join(" ")
                )));
            }
        };
        Ok(command)
    }
}

/// Parses the options of `fuzz`: a target name and `--seconds <n>`, in any
/// order.
fn parse_fuzz(words: &[&str]) -> Result<Command> {
    let mut target = None;
    let mut seconds = fuzz::DEFAULT_SECONDS;
    let mut words = words.iter();
    while let Some(word) = words.next() {
        if *word == "--seconds" {
            seconds = words
                .next()
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| Error::Usage("fuzz: --seconds needs a number".to_owned()))?;
        } else if let Some(name) = fuzz::TARGETS.iter().find(|name| *name == word) {
            target = Some(*name);
        } else {
            return Err(Error::Usage(format!(
                "fuzz: unknown target `{word}`; the targets are {}",
                fuzz::TARGETS.join(", ")
            )));
        }
    }
    Ok(Command::Fuzz { target, seconds })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match Command::parse(&args).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            let mut source = std::error::Error::source(&error);
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            ExitCode::FAILURE
        }
    }
}

/// Runs `command`.
fn run(command: Command) -> Result<()> {
    let root = workspace_root();
    match command {
        Command::Help => {
            print!("{USAGE}");
            Ok(())
        }
        Command::Lint { strict: false } => lint::run(&Workspace::load(&root)?),
        Command::Lint { strict: true } => lint::strict(&Workspace::load(&root)?),
        Command::Test => cargo::test(&root),
        Command::Bless => cargo::bless(&Workspace::load(&root)?),
        Command::RecordParity { check } => cargo::record_parity(&root, check),
        Command::Doc { open } => cargo::doc(&root, open),
        Command::CheckDeps { bless } => deps::run(&Workspace::load(&root)?, bless),
        Command::Metrics(mode) => metrics::run(&Workspace::load(&root)?, mode),
        Command::Coverage(mode) => coverage::run(&Workspace::load(&root)?, mode),
        Command::Fuzz { target, seconds } => fuzz::run(&root, target, seconds),
        Command::Widths => lint::check_widths(&root),
    }
}

/// The workspace root: the parent of this crate's directory.
fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map_or_else(|| manifest_dir.to_path_buf(), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<Command> {
        let args: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
        Command::parse(&args)
    }

    #[test]
    fn commands_and_their_options_parse() {
        assert_eq!(parse("").unwrap(), Command::Help);
        assert_eq!(
            parse("lint --strict").unwrap(),
            Command::Lint { strict: true }
        );
        assert_eq!(
            parse("metrics --check").unwrap(),
            Command::Metrics(Mode::Check)
        );
        assert_eq!(
            parse("check-deps --bless").unwrap(),
            Command::CheckDeps { bless: true }
        );
        assert_eq!(parse("bless").unwrap(), Command::Bless);
        assert_eq!(
            parse("record-parity --check").unwrap(),
            Command::RecordParity { check: true }
        );
        assert_eq!(
            parse("fuzz crypttab --seconds 5").unwrap(),
            Command::Fuzz {
                target: Some("crypttab"),
                seconds: 5
            }
        );
        assert_eq!(
            parse("fuzz").unwrap(),
            Command::Fuzz {
                target: None,
                seconds: 60
            }
        );
        assert!(parse("fuzz nothing").is_err());
        assert!(parse("fuzz --seconds x").is_err());
        assert_eq!(
            parse("coverage --check").unwrap(),
            Command::Coverage(Mode::Check)
        );
    }

    #[test]
    fn an_unknown_command_says_how_to_get_help() {
        let error = parse("lint --fast").unwrap_err();

        assert_eq!(
            error.to_string(),
            "unknown command `lint --fast`; run `cargo xtask help`"
        );
    }

    #[test]
    fn the_root_is_the_workspace_root() {
        assert!(workspace_root().join("Cargo.lock").is_file());
    }
}
