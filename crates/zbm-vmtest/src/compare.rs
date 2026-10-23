//! `zbm-vmtest compare`: the checks of two runs side by side, such as those
//! of two commits, message by message.
//!
//! Each test's checks are read from its `steps.log`. A check of one run
//! matches the first unmatched check of the other run with the same
//! message; the differences are the checks without a match, and matched
//! checks that passed in one run and failed in the other. Messages that
//! hold values that change from run to run (UUIDs, sizes) show up as a
//! check missing on each side.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// One check of a run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Check {
    /// Whether it passed.
    pub passed: bool,
    /// Its message.
    pub message: String,
}

/// A difference between the checks of two runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Difference {
    /// A check of the first run without a match in the second.
    OnlyInFirst(Check),
    /// A check of the second run without a match in the first.
    OnlyInSecond(Check),
    /// The same check passed in one run and failed in the other.
    Verdict {
        /// The message.
        message: String,
        /// Whether it passed in the first run.
        first_passed: bool,
    },
}

/// The checks in the text of a `steps.log`.
pub fn checks(steps: &str) -> Vec<Check> {
    steps
        .lines()
        .filter_map(|line| {
            // "[HH:MM:SS] ok   - message"
            let rest = line.split_once("] ").map(|(_, rest)| rest)?;
            if let Some(message) = rest.strip_prefix("ok   - ") {
                Some(Check {
                    passed: true,
                    message: message.to_owned(),
                })
            } else {
                rest.strip_prefix("FAIL - ").map(|message| Check {
                    passed: false,
                    message: message.to_owned(),
                })
            }
        })
        .collect()
}

/// The differences between the checks `first` and `second`.
pub fn differences(first: &[Check], second: &[Check]) -> Vec<Difference> {
    let mut matched = vec![false; second.len()];
    let mut differences = Vec::new();
    for check in first {
        let found = second
            .iter()
            .enumerate()
            .position(|(index, other)| !matched[index] && other.message == check.message);
        match found {
            Some(index) => {
                matched[index] = true;
                if second[index].passed != check.passed {
                    differences.push(Difference::Verdict {
                        message: check.message.clone(),
                        first_passed: check.passed,
                    });
                }
            }
            None => differences.push(Difference::OnlyInFirst(check.clone())),
        }
    }
    let unmatched = second
        .iter()
        .zip(&matched)
        .filter(|(_, matched)| !**matched);
    differences.extend(unmatched.map(|(check, _)| Difference::OnlyInSecond(check.clone())));
    differences
}

/// The verdict of a check, as `steps.log` writes it.
fn verdict(passed: bool) -> &'static str {
    if passed { "ok  " } else { "FAIL" }
}

/// The differences as lines of a report.
pub fn report(differences: &[Difference]) -> String {
    let mut text = String::new();
    for difference in differences {
        match difference {
            Difference::OnlyInFirst(check) => {
                writeln!(text, "    < {} - {}", verdict(check.passed), check.message)
            }
            Difference::OnlyInSecond(check) => {
                writeln!(text, "    > {} - {}", verdict(check.passed), check.message)
            }
            Difference::Verdict {
                message,
                first_passed,
            } => writeln!(
                text,
                "    ! {} / {} - {message}",
                verdict(*first_passed),
                verdict(!*first_passed)
            ),
        }
        .ok();
    }
    text
}

/// Compares the tests `tests` (all of either run when empty) of the work
/// directories `first` and `second`; prints a report and returns whether
/// they have the same checks with the same verdicts.
pub fn has_same_checks(first: &Path, second: &Path, tests: &[String]) -> bool {
    let tests = if tests.is_empty() {
        test_names(first, second)
    } else {
        tests.to_vec()
    };
    println!("< {}", first.display());
    println!("> {}", second.display());
    let mut same = true;
    for test in &tests {
        let read = |work: &Path| {
            let path = work.join("tests").join(test).join("steps.log");
            fs::read(path)
                .ok()
                .map(|bytes| checks(&String::from_utf8_lossy(&bytes)))
        };
        match (read(first), read(second)) {
            (Some(a), Some(b)) => {
                let found = differences(&a, &b);
                if found.is_empty() {
                    println!("  {test:28} same ({} checks)", a.len());
                } else {
                    same = false;
                    println!("  {test:28} {} differences", found.len());
                    print!("{}", report(&found));
                }
            }
            (a, b) => {
                same = false;
                let missing = if a.is_none() { "<" } else { ">" };
                let missing = if a.is_none() && b.is_none() {
                    "both"
                } else {
                    missing
                };
                println!("  {test:28} no steps.log ({missing})");
            }
        }
    }
    same
}

/// The tests that have a directory in either work directory, sorted.
fn test_names(first: &Path, second: &Path) -> Vec<String> {
    let mut names: Vec<String> = [first, second]
        .iter()
        .filter_map(|work| fs::read_dir(work.join("tests")).ok())
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn check(passed: bool, message: &str) -> Check {
        Check {
            passed,
            message: message.into(),
        }
    }

    #[test]
    fn checks_are_read_from_the_steps() {
        let steps = "[10:00:00] test x: ZBMTEST_WORK=/w\n[10:00:01] ok   - zpool create t\n\
                     [10:00:02]       FAIL - shown output\n[10:00:03] FAIL - kexec\n\
                     [10:00:04] note - n\n";

        assert_eq!(
            checks(steps),
            [check(true, "zpool create t"), check(false, "kexec")]
        );
    }

    #[test]
    fn differences_are_missing_checks_and_changed_verdicts() {
        let first = [
            check(true, "a"),
            check(true, "b"),
            check(true, "a"),
            check(false, "uuid 1"),
        ];
        let second = [
            check(true, "a"),
            check(false, "b"),
            check(false, "uuid 2"),
            check(true, "a"),
        ];

        let found = differences(&first, &second);

        assert_eq!(
            found,
            [
                Difference::Verdict {
                    message: "b".into(),
                    first_passed: true
                },
                Difference::OnlyInFirst(check(false, "uuid 1")),
                Difference::OnlyInSecond(check(false, "uuid 2")),
            ]
        );
        assert_eq!(
            report(&found),
            "    ! ok   / FAIL - b\n    < FAIL - uuid 1\n    > FAIL - uuid 2\n"
        );
    }

    #[test]
    fn two_runs_are_compared_test_by_test() {
        let settings = crate::testing::settings("compare");
        let write = |run: &str, test: &str, steps: &str| {
            let dir = settings.work.join(run).join("tests").join(test);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("steps.log"), steps).unwrap();
        };
        write("py", "keys-plugin", "[1] ok   - a\n");
        write("rs", "keys-plugin", "[2] ok   - a\n");
        write("py", "keys-notpm", "[1] ok   - a\n");
        let (py, rs) = (settings.work.join("py"), settings.work.join("rs"));

        assert!(has_same_checks(&py, &rs, &["keys-plugin".to_owned()]));
        assert!(!has_same_checks(&py, &rs, &[]));
        assert_eq!(test_names(&py, &rs), ["keys-notpm", "keys-plugin"]);
        crate::testing::cleanup(&settings);
    }
}
