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
mod debuginfo;
mod deps;
mod error;
mod fuzz;
mod gen_docs;
mod lint;
mod metrics;
mod process;
mod record_zfs;
mod texts;
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
                         xtask/strict/crates.txt, documentation,
                         gen-docs --check, check-deps and metrics --check
  lint --strict          Report the stricter lints that refactored crates
                         must pass, for every crate; fails only on a finding
                         in a crate of xtask/strict/crates.txt
  test                   Every test that needs no root and no virtual
                         machine, under cargo nextest when it is installed
  test devices           The LUKS device suites: the LUKS code against real
                         LUKS volumes on loop devices (root, cryptsetup,
                         swtpm, clevis, GnuPG; the build container has them)
  test vm [<test>...]    The VM tests: build zbm-vmtest and run the tests
                         (all by default) in QEMU (KVM, the build container,
                         root for the disk setups; docs/luks/testing.rst)
  bless                  Accept every changed snapshot and list the files
  record-parity [--check]
                         Run the legacy code of the parity tests and record
                         its results in tests/golden/legacy; --check fails
                         where a recording no longer matches
  record-zfs [--manifest]
                         Record what zfs and zpool print, in a virtual
                         machine of the VM test harness (root, QEMU), as the
                         fixtures of crates/zbm-zfs/tests/fixtures;
                         --manifest prints the commands the harness runs
  doc [--open]           Build the documentation, warnings as errors
  gen-docs [--check]     Write the option sections of zfsbootmenu(7),
                         zfsbootmenu-crypttab(5) and generate-zbm(5) from
                         the table of options in zbm-config; --check fails
                         when one is out of date
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
  split-debug <binary> [<dir>]
                         Move the debug information of a release binary to
                         <dir>/<build-id>.debug (target/debug-files) and
                         strip the binary, as images are built
  symbolize <report> [--debug-dir <dir>]...
                         Print a crash report of the runtime with function
                         names, files and lines, from the debug files of its
                         build ID (target/debug-files)
  widths                 Check the line widths of The Rust Style Guide
  help                   Show this text
";

/// One command of the command line.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    /// `lint`, strict or not.
    Lint { strict: bool },
    /// `test`.
    Test,
    /// `test devices`.
    TestDevices,
    /// `test vm`, with the tests to run.
    TestVm(Vec<String>),
    /// `bless`.
    Bless,
    /// `record-parity`.
    RecordParity { check: bool },
    /// `record-zfs`, or only its manifest.
    RecordZfs { manifest: bool },
    /// `doc`.
    Doc { open: bool },
    /// `gen-docs`.
    GenDocs { check: bool },
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
    /// `split-debug`: the binary and the directory of debug files.
    SplitDebug { binary: PathBuf, dir: PathBuf },
    /// `symbolize`: the report and the directories of debug files.
    Symbolize { report: PathBuf, dirs: Vec<PathBuf> },
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
            ["test", "devices"] => Command::TestDevices,
            ["test", "vm", tests @ ..] => {
                Command::TestVm(tests.iter().map(|&test| test.to_owned()).collect())
            }
            ["bless"] => Command::Bless,
            ["record-parity"] => Command::RecordParity { check: false },
            ["record-parity", "--check"] => Command::RecordParity { check: true },
            ["record-zfs"] => Command::RecordZfs { manifest: false },
            ["record-zfs", "--manifest"] => Command::RecordZfs { manifest: true },
            ["doc"] => Command::Doc { open: false },
            ["doc", "--open"] => Command::Doc { open: true },
            ["gen-docs"] => Command::GenDocs { check: false },
            ["gen-docs", "--check"] => Command::GenDocs { check: true },
            ["check-deps"] => Command::CheckDeps { bless: false },
            ["check-deps", "--bless"] => Command::CheckDeps { bless: true },
            ["metrics"] => Command::Metrics(Mode::Report),
            ["metrics", "--check"] => Command::Metrics(Mode::Check),
            ["metrics", "--bless"] => Command::Metrics(Mode::Bless),
            ["fuzz", rest @ ..] => parse_fuzz(rest)?,
            ["coverage"] => Command::Coverage(Mode::Report),
            ["coverage", "--check"] => Command::Coverage(Mode::Check),
            ["coverage", "--bless"] => Command::Coverage(Mode::Bless),
            ["split-debug", binary] => Command::SplitDebug {
                binary: PathBuf::from(binary),
                dir: PathBuf::from(debuginfo::DEFAULT_DIR),
            },
            ["split-debug", binary, dir] => Command::SplitDebug {
                binary: PathBuf::from(binary),
                dir: PathBuf::from(dir),
            },
            ["symbolize", report, rest @ ..] => parse_symbolize(report, rest)?,
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

/// Parses the options of `symbolize`: `--debug-dir <dir>`, any number of
/// times; without one, the default directory.
fn parse_symbolize(report: &str, words: &[&str]) -> Result<Command> {
    let mut dirs = Vec::new();
    let mut words = words.iter();
    while let Some(word) = words.next() {
        match (*word, words.next()) {
            ("--debug-dir", Some(dir)) => dirs.push(PathBuf::from(dir)),
            _ => {
                return Err(Error::Usage(
                    "symbolize: the options are --debug-dir <dir>".to_owned(),
                ));
            }
        }
    }
    if dirs.is_empty() {
        dirs.push(PathBuf::from(debuginfo::DEFAULT_DIR));
    }
    Ok(Command::Symbolize {
        report: PathBuf::from(report),
        dirs,
    })
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
        Command::TestDevices => cargo::test_devices(&root),
        Command::TestVm(tests) => cargo::test_vm(&root, &tests),
        Command::Bless => cargo::bless(&Workspace::load(&root)?),
        Command::RecordParity { check } => cargo::record_parity(&root, check),
        Command::RecordZfs { manifest: true } => {
            println!("{}", record_zfs::manifest());
            Ok(())
        }
        Command::RecordZfs { manifest: false } => {
            cargo::test_vm(&root, &["runtime-record-zfs".to_owned()])
        }
        Command::Doc { open } => cargo::doc(&root, open),
        Command::GenDocs { check } => gen_docs::run(&root, check),
        Command::CheckDeps { bless } => deps::run(&Workspace::load(&root)?, bless),
        Command::Metrics(mode) => metrics::run(&Workspace::load(&root)?, mode),
        Command::Coverage(mode) => coverage::run(&Workspace::load(&root)?, mode),
        Command::Fuzz { target, seconds } => fuzz::run(&root, target, seconds),
        Command::SplitDebug { binary, dir } => debuginfo::split(&root, &binary, &dir),
        Command::Symbolize { report, dirs } => debuginfo::symbolize(&root, &report, &dirs),
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
        assert_eq!(parse("test devices").unwrap(), Command::TestDevices);
        assert_eq!(
            parse("test vm procedures").unwrap(),
            Command::TestVm(vec!["procedures".to_owned()])
        );
        assert_eq!(
            parse("record-parity --check").unwrap(),
            Command::RecordParity { check: true }
        );
        assert_eq!(
            parse("record-zfs --manifest").unwrap(),
            Command::RecordZfs { manifest: true }
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
