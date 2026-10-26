//! `cargo xtask fuzz`: runs the fuzz targets of `fuzz/` with `cargo fuzz`.
//!
//! The targets feed arbitrary bytes to the parsers of untrusted input that
//! `docs/dev/threat-model.md` lists. Each run starts from the committed seeds
//! in `fuzz/seeds/<target>` and grows a corpus in `fuzz/corpus/<target>`;
//! inputs that make a target fail land in `fuzz/artifacts/<target>`. Fuzzing
//! needs nightly Rust and `cargo-fuzz` (`docs/dev/testing.md`).

use std::path::Path;

use crate::cargo;
use crate::error::Result;
use crate::process::Program;

/// The fuzz targets, as `fuzz/Cargo.toml` names them.
pub const TARGETS: &[&str] = &[
    "kernel_cmdline",
    "domain_names",
    "crypttab",
    "luks_metadata",
    "program_output",
    "display_data",
    "console_output",
];

/// How long each target runs when no time is given, in seconds.
pub const DEFAULT_SECONDS: u64 = 60;

/// Runs `target`, or every target, for `seconds` each.
///
/// # Errors
///
/// When a target finds an input that makes it fail, or cannot be built.
pub fn run(root: &Path, target: Option<&str>, seconds: u64) -> Result<()> {
    let targets = target.map_or_else(|| TARGETS.to_vec(), |target| vec![target]);
    for target in targets {
        std::fs::create_dir_all(root.join("fuzz/corpus").join(target)).map_err(|error| {
            crate::error::Error::io(format!("creating fuzz/corpus/{target}"), error)
        })?;
        program(root, target, seconds).run()?;
    }
    println!("fuzz: no target failed");
    Ok(())
}

/// The run of one target.
fn program(root: &Path, target: &str, seconds: u64) -> Program {
    cargo::cargo(root)
        .args(["+nightly", "fuzz", "run", target])
        .args([
            format!("fuzz/corpus/{target}"),
            format!("fuzz/seeds/{target}"),
        ])
        .args(["--", &format!("-max_total_time={seconds}")])
        .args(["-timeout=10", "-rss_limit_mb=2048"])
}

/// Checks that the fuzz targets build, so that moving a parser does not
/// break them unnoticed. They build with the normal compiler; only running
/// them needs nightly.
///
/// # Errors
///
/// When a target does not build.
pub fn check(root: &Path) -> Result<()> {
    cargo::cargo(root)
        .args(["check", "--manifest-path", "fuzz/Cargo.toml", "--locked"])
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_runs_from_its_seeds_with_a_time_limit() {
        let program = program(Path::new("/w"), "crypttab", 30);

        assert_eq!(
            program.shown(),
            "cargo +nightly fuzz run crypttab fuzz/corpus/crypttab fuzz/seeds/crypttab -- \
             -max_total_time=30 -timeout=10 -rss_limit_mb=2048"
        );
    }

    #[test]
    fn the_targets_are_those_of_the_fuzz_crate() {
        let manifest =
            std::fs::read_to_string(crate::workspace_root().join("fuzz/Cargo.toml")).unwrap();
        let names: Vec<&str> = manifest
            .lines()
            .filter_map(|line| line.strip_prefix("name = \""))
            .filter_map(|rest| rest.strip_suffix('"'))
            .filter(|name| *name != "zbm-fuzz")
            .collect();

        assert_eq!(names, TARGETS);
        for target in TARGETS {
            let seeds = crate::workspace_root().join("fuzz/seeds").join(target);
            assert!(seeds.is_dir(), "{target} has no seeds");
        }
    }
}
