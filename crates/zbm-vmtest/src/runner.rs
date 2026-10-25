//! `zbm-vmtest run`: the tests one after another, each in a process of its
//! own, then a summary of their results.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use crate::disks::payload_disk;
use crate::error::{Error, Result};
use crate::images::build_be;
use crate::record::Outcome;
use crate::settings::{Settings, ZbmRuntime};

/// The name of a test's result directory: with Rust images, tests that do
/// not choose their runtime end in `-rust`.
pub fn result_name(settings: &Settings, test: &str) -> String {
    if settings.runtime == ZbmRuntime::Rust && !test.starts_with("runtime-") {
        format!("{test}-rust")
    } else {
        test.to_owned()
    }
}

/// The file that gets the output of a test's process.
fn output_file(settings: &Settings, test: &str) -> PathBuf {
    let suffix = std::env::var("ZBMTEST_ZBM_RUNTIME")
        .ok()
        .filter(|runtime| !runtime.is_empty())
        .map_or_else(String::new, |runtime| format!("-{runtime}"));
    settings.work.join(format!("run-{test}{suffix}.out"))
}

/// The last line of a file, if it has one.
fn last_line(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    text.lines().last().map(str::to_owned)
}

/// Runs `tests` in order, each with `program scenario <test>` (this program),
/// and prints a summary; returns whether every test passed.
///
/// # Errors
///
/// When the boot environment or the payload disk cannot be built, or a
/// test's process cannot be started.
pub fn run(settings: &Settings, program: &Path, tests: &[String]) -> Result<bool> {
    fs::create_dir_all(&settings.work).map_err(Error::io("creating the work directory"))?;
    // Results older than this run are not reported: a test that failed
    // before it wrote one would otherwise show the last run's result.
    let started = SystemTime::now();
    build_be(settings)?;
    payload_disk(settings)?;
    let mut all_passed = true;
    for test in tests {
        println!("=== {test} ({})", crate::record::clock());
        let out = output_file(settings, test);
        let file = File::create(&out).map_err(Error::io(format!("creating {}", out.display())))?;
        let errors = file
            .try_clone()
            .map_err(Error::io("opening the test's output"))?;
        let status = Command::new(program)
            .args(["scenario", test])
            .stdin(Stdio::null())
            .stdout(file)
            .stderr(errors)
            .status()
            .map_err(Error::io(format!("starting {}", program.display())))?;
        println!("{}", last_line(&out).unwrap_or_default());
        all_passed &= status.success();
    }
    let results: Vec<(String, Option<Outcome>)> = tests
        .iter()
        .map(|test| {
            let name = result_name(settings, test);
            let path = settings.work.join("tests").join(&name).join("result.json");
            (name, read_result(&path, started))
        })
        .collect();
    println!();
    println!("Summary ({}/tests/*/result.json):", settings.work.display());
    let (text, failed) = summary(&results);
    print!("{text}");
    Ok(all_passed && failed == 0)
}

/// The result at `path`, if it was written after `since`.
fn read_result(path: &Path, since: SystemTime) -> Option<Outcome> {
    let modified = fs::metadata(path).and_then(|meta| meta.modified()).ok()?;
    if modified < since {
        return None;
    }
    Outcome::from_json(&fs::read_to_string(path).ok()?)
}

/// The summary of the results, as the Python harness printed it, and the
/// number of failed checks (a test without a result counts as one).
pub fn summary(results: &[(String, Option<Outcome>)]) -> (String, usize) {
    let mut text = String::new();
    let (mut passed, mut failed) = (0, 0);
    for (name, outcome) in results {
        let Some(outcome) = outcome else {
            writeln!(text, "  {name:22} no result").ok();
            failed += 1;
            continue;
        };
        passed += outcome.passed;
        failed += outcome.failures.len();
        let verdict = outcome.verdict();
        writeln!(
            text,
            "  {name:22} {verdict}  {:3} passed  {:2} failed  {:5} s",
            outcome.passed,
            outcome.failures.len(),
            outcome.seconds
        )
        .ok();
    }
    writeln!(
        text,
        "  {:22}       {passed:3} passed  {failed:2} failed",
        "total"
    )
    .ok();
    (text, failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_summary_lists_each_test_and_the_total() {
        let pass = Outcome {
            test: "layouts-main".into(),
            passed: 61,
            seconds: 412,
            ..Outcome::default()
        };
        let fail = Outcome {
            test: "keys-plugin".into(),
            passed: 20,
            failures: vec!["x".into(), "y".into()],
            seconds: 98,
            ..Outcome::default()
        };
        let skipped = Outcome {
            test: "layouts-main-oldzfs".into(),
            skipped: vec!["no old image".into()],
            ..Outcome::default()
        };
        let results = vec![
            ("layouts-main".to_owned(), Some(pass)),
            ("keys-plugin".to_owned(), Some(fail)),
            ("keys-setup".to_owned(), None),
            ("layouts-main-oldzfs".to_owned(), Some(skipped)),
        ];

        let (text, failed) = summary(&results);

        assert_eq!(failed, 3);
        assert_eq!(
            text,
            "  layouts-main           PASS   61 passed   0 failed    412 s\n\
             \x20 keys-plugin            FAIL   20 passed   2 failed     98 s\n\
             \x20 keys-setup             no result\n\
             \x20 layouts-main-oldzfs    SKIP    0 passed   0 failed      0 s\n\
             \x20 total                         81 passed   3 failed\n"
        );
    }

    #[test]
    fn results_of_rust_images_are_named_apart() {
        let mut settings = Settings::from_vars(|_| None, false, false);
        settings.runtime = ZbmRuntime::Rust;

        assert_eq!(
            result_name(&settings, "complex-healthy"),
            "complex-healthy-rust"
        );
        assert_eq!(result_name(&settings, "runtime-legacy"), "runtime-legacy");
    }

    #[test]
    fn a_result_older_than_the_run_is_not_read() {
        let settings = crate::testing::settings("runner-old");
        let path = settings.work.join("result.json");
        fs::create_dir_all(&settings.work).unwrap();
        let outcome = Outcome {
            test: "procedures".into(),
            ..Outcome::default()
        };
        fs::write(&path, outcome.to_json()).unwrap();

        let later = SystemTime::now() + std::time::Duration::from_secs(60);

        assert_eq!(read_result(&path, SystemTime::UNIX_EPOCH), Some(outcome));
        assert_eq!(read_result(&path, later), None);
        assert_eq!(last_line(&path).as_deref(), Some("}"));
        crate::testing::cleanup(&settings);
    }
}
