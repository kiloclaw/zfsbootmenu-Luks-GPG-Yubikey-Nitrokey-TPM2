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
    test_with(root, &[])
}

/// Runs the tests as [`test()`] does, with the environment variables `env`.
fn test_with(root: &Path, env: &[(&str, &str)]) -> Result<()> {
    let nextest = has_nextest(root);
    if !nextest {
        println!("test: cargo-nextest is not installed; using cargo test");
    }
    for program in test_programs(root, nextest, env) {
        program.run()?;
    }
    Ok(())
}

/// The programs [`test()`] runs, with or without `cargo nextest`, with the
/// environment variables `env`.
fn test_programs(root: &Path, nextest: bool, env: &[(&str, &str)]) -> Vec<Program> {
    let program = |args: &[&str]| {
        let mut program = cargo(root);
        for (key, value) in env {
            program = program.env(key, value);
        }
        program.args(args)
    };
    let runner: &[&str] = if nextest {
        &["nextest", "run"]
    } else {
        &["test"]
    };
    let mut programs = vec![program(runner).args(["--workspace", "--locked"])];
    if nextest {
        programs.push(program(&["test", "--workspace", "--doc", "--locked"]));
    }
    programs.push(program(runner).args(["-p", "zbm", "--features", "test-seams", "--locked"]));
    programs
}

/// Runs the LUKS device suites of `crates/zbm-luks/tests/devices`, which
/// replace the Bash suites of `tests/luks/unit`: as root, with real LUKS
/// volumes on loop devices, a software TPM, clevis and GnuPG. The suites
/// run one after another and print every case and check.
///
/// # Errors
///
/// When a check fails, or the suites cannot run here.
pub fn test_devices(root: &Path) -> Result<()> {
    test_devices_program(root).run()
}

/// The test run of [`test_devices`].
fn test_devices_program(root: &Path) -> Program {
    cargo(root).env("ZBM_DEVICE_TESTS", "1").args([
        "test",
        "-p",
        "zbm-luks",
        "--test",
        "devices",
        "--locked",
        "--",
        "--nocapture",
        "--test-threads=1",
    ])
}

/// Builds `zbm-vmtest` as the user and runs the VM tests `tests` (all of
/// them when empty) as root, with `sudo` unless xtask already runs as root:
/// ZFSBootMenu images booted in QEMU, which needs KVM, the build container
/// and, for the disk and key setups, root (`docs/luks/testing.rst`). The
/// harness's settings, the `ZBMTEST_*` variables, are passed through
/// `env`: `sudo-rs` ignores `sudo -E`.
///
/// # Errors
///
/// When the build or a test fails.
pub fn test_vm(root: &Path, tests: &[String]) -> Result<()> {
    let settings: Vec<(String, String)> = std::env::vars()
        .filter(|(name, _)| name.starts_with("ZBMTEST_"))
        .collect();
    for program in test_vm_programs(root, tests, is_root(), &settings) {
        program.run()?;
    }
    Ok(())
}

/// The programs [`test_vm`] runs; `as_root` when xtask runs as root, with
/// the harness's `settings` to pass through `sudo` otherwise.
fn test_vm_programs(
    root: &Path,
    tests: &[String],
    as_root: bool,
    settings: &[(String, String)],
) -> Vec<Program> {
    let build = cargo(root).args(["build", "--release", "--locked", "-p", "zbm-vmtest"]);
    let harness = root.join("target/release/zbm-vmtest").display().to_string();
    let run = if as_root {
        Program::new(&harness, root)
    } else {
        let assignments = settings
            .iter()
            .map(|(name, value)| format!("{name}={value}"));
        Program::new("sudo", root)
            .args(["env".to_owned()])
            .args(assignments)
            .args([harness.clone()])
    };
    vec![build, run.args(["run"]).args(tests)]
}

/// Whether this process runs as root: its effective user ID, the second
/// number of the `Uid:` line of `/proc/self/status`, is 0.
fn is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .is_ok_and(|status| effective_uid(&status) == Some(0))
}

/// The effective user ID in the text of `/proc/<pid>/status`.
fn effective_uid(status: &str) -> Option<u32> {
    let line = status.lines().find_map(|line| line.strip_prefix("Uid:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// The golden files of the legacy results, relative to the root.
pub const GOLDEN_LEGACY: &str = "tests/golden/legacy";

/// Records the results of the legacy code that parity tests compare with:
/// runs the tests with `ZBM_PARITY=record`, which runs the legacy code and
/// writes the golden files below [`GOLDEN_LEGACY`], then lists the files that
/// changed. With `check`, runs them with `ZBM_PARITY=check` instead, which
/// fails where the legacy code no longer gives the recorded result.
///
/// Recording needs what the legacy code needs: Bash, Perl with its modules,
/// binutils and the systemd EFI stub (`docs/dev/testing.md`). Run it as root
/// to record the cases that compare with the legacy code only as root.
///
/// # Errors
///
/// When a test fails.
pub fn record_parity(root: &Path, check: bool) -> Result<()> {
    let mode = if check { "check" } else { "record" };
    test_with(root, &[("ZBM_PARITY", mode)])?;
    if check {
        println!("record-parity: every recording matches the legacy code");
        return Ok(());
    }
    let changed = git_changes(root, GOLDEN_LEGACY)?;
    print!("{}", changes("record-parity", "recordings", &changed));
    Ok(())
}

/// What `git status --short` says about the files below `pathspec`.
fn git_changes(root: &Path, pathspec: &str) -> Result<String> {
    Program::new("git", root)
        .args(["status", "--short", "--", pathspec])
        .output()
}

/// The report of a command that rewrote `what`, from `git status` output.
fn changes(command: &str, what: &str, changed: &str) -> String {
    if changed.is_empty() {
        format!("{command}: no {} changed\n", what.trim_end_matches('s'))
    } else {
        format!("{command}: these {what} changed; review them with `git diff`:\n{changed}")
    }
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
    bless_program(workspace).run()?;
    let changed = git_changes(&workspace.root, "*.snap")?;
    print!("{}", changes("bless", "snapshots", &changed));
    Ok(())
}

/// The test run of [`bless`]: the crates with snapshot tests.
fn bless_program(workspace: &Workspace) -> Program {
    let mut args = vec!["test".to_owned(), "--locked".to_owned()];
    for package in snapshot_packages(workspace) {
        args.push("-p".to_owned());
        args.push(package.to_owned());
    }
    cargo(&workspace.root)
        .args(args)
        .env("INSTA_UPDATE", "always")
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

/// Checks that `rustfmt` would change nothing, in the workspace and in the
/// fuzz targets.
///
/// # Errors
///
/// When a file is not formatted.
pub fn fmt_check(root: &Path) -> Result<()> {
    cargo(root).args(["fmt", "--all", "--check"]).run()?;
    cargo(root)
        .args(["fmt", "--manifest-path", "fuzz/Cargo.toml", "--check"])
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{Dependency, DependencyKind, Package};
    use std::path::PathBuf;

    fn shown(programs: &[Program]) -> Vec<&str> {
        programs.iter().map(Program::shown).collect()
    }

    #[test]
    fn tests_run_under_nextest_with_the_doctests_apart() {
        let programs = test_programs(Path::new("/w"), true, &[("ZBM_PARITY", "check")]);

        assert_eq!(
            shown(&programs),
            [
                "ZBM_PARITY=check cargo nextest run --workspace --locked",
                "ZBM_PARITY=check cargo test --workspace --doc --locked",
                "ZBM_PARITY=check cargo nextest run -p zbm --features test-seams --locked",
            ]
        );
    }

    #[test]
    fn vm_tests_run_with_a_release_build_of_the_harness() {
        let tests = vec!["keys-setup".to_owned(), "keys-plugin".to_owned()];

        let settings = [("ZBMTEST_REUSE".to_owned(), "1".to_owned())];
        let programs = test_vm_programs(Path::new("/w"), &tests, true, &settings);
        let as_user = test_vm_programs(Path::new("/w"), &tests, false, &settings);

        assert_eq!(
            shown(&programs),
            [
                "cargo build --release --locked -p zbm-vmtest",
                "/w/target/release/zbm-vmtest run keys-setup keys-plugin",
            ]
        );
        // The harness runs as root with its settings; the build stays the
        // user's.
        assert_eq!(
            shown(&as_user),
            [
                "cargo build --release --locked -p zbm-vmtest",
                "sudo env ZBMTEST_REUSE=1 /w/target/release/zbm-vmtest run keys-setup keys-plugin",
            ]
        );
    }

    #[test]
    fn root_is_an_effective_user_id_of_0() {
        let user = "Name:\txtask\nUid:\t1000\t1000\t1000\t1000\n";
        let setuid = "Uid:\t1000\t0\t0\t0\n";

        assert_eq!(effective_uid(user), Some(1000));
        assert_eq!(effective_uid(setuid), Some(0));
        assert_eq!(effective_uid("Name:\txtask\n"), None);
        assert_eq!(
            is_root(),
            effective_uid(&std::fs::read_to_string("/proc/self/status").unwrap()) == Some(0)
        );
    }

    #[test]
    fn without_nextest_cargo_test_runs_everything() {
        let programs = test_programs(Path::new("/w"), false, &[]);

        assert_eq!(
            shown(&programs),
            [
                "cargo test --workspace --locked",
                "cargo test -p zbm --features test-seams --locked",
            ]
        );
    }

    #[test]
    fn the_device_suites_run_one_after_another() {
        assert_eq!(
            test_devices_program(Path::new("/w")).shown(),
            "ZBM_DEVICE_TESTS=1 cargo test -p zbm-luks --test devices --locked -- \
             --nocapture --test-threads=1"
        );
    }

    #[test]
    fn bless_runs_the_crates_with_snapshots() {
        let package = |name: &str, dependencies: &[&str]| Package {
            name: name.to_owned(),
            dir: PathBuf::from(format!("/w/crates/{name}")),
            targets: Vec::new(),
            dependencies: dependencies
                .iter()
                .map(|name| Dependency {
                    name: (*name).to_owned(),
                    kind: DependencyKind::Dev,
                    member: false,
                })
                .collect(),
        };
        let workspace = Workspace {
            root: PathBuf::from("/w"),
            packages: vec![
                package("zbm-tui", &["insta"]),
                package("zbm", &["proptest"]),
            ],
        };

        assert_eq!(
            bless_program(&workspace).shown(),
            "INSTA_UPDATE=always cargo test --locked -p zbm-tui"
        );
    }

    #[test]
    fn changes_are_listed_for_review() {
        assert_eq!(
            changes("bless", "snapshots", ""),
            "bless: no snapshot changed\n"
        );
        assert_eq!(
            changes("record-parity", "recordings", " M a.json\n"),
            "record-parity: these recordings changed; review them with `git diff`:\n M a.json\n"
        );
    }
}
