//! Assertions that say what went wrong in the words of the test.
//!
//! - [`assert_ran!`](crate::assert_ran) and
//!   [`assert_not_ran!`](crate::assert_not_ran) look for a command among
//!   those run, and list them all when the check fails.
//! - [`assert_no_secret_leaked!`](crate::assert_no_secret_leaked) checks that
//!   secrets went nowhere they must not: arguments, the environment, plain
//!   standard input, files (the kernel log and the journal among them) and
//!   the console.
//! - [`cases!`](crate::cases) runs a table of named cases and names every
//!   case that fails, not only the first.
//!
//! ```
//! use zbm_testkit::fakes::FakeSystem;
//! use zbm_testkit::{assert_not_ran, assert_ran, cases};
//! use zbm_core::exec::{Command, Runner};
//!
//! let fake = FakeSystem::new();
//! fake.runner.run(&Command::new("zpool").args(["import", "-a"])).unwrap();
//! assert_ran!(fake, "zpool import -a");
//! assert_not_ran!(fake, "zpool export");
//!
//! cases! {
//!     for (text, length) in {
//!         "empty" => ("", 0),
//!         "a word" => ("tank", 4),
//!     } {
//!         assert_eq!(text.len(), length);
//!     }
//! }
//! ```

use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// The commands `lines` as a numbered list, for a failure message.
pub fn command_list(lines: &[String]) -> String {
    if lines.is_empty() {
        return "  (no command ran)\n".to_owned();
    }
    lines
        .iter()
        .enumerate()
        .fold(String::new(), |mut list, (index, line)| {
            let _ = writeln!(list, "  {index}: {line}");
            list
        })
}

/// Fails the test unless one of `lines` is `expected`.
#[track_caller]
pub fn ran(lines: &[String], expected: &str) {
    assert!(
        lines.iter().any(|line| line == expected),
        "`{expected}` did not run; the commands that ran:\n{}",
        command_list(lines)
    );
}

/// Fails the test if one of `lines` starts with `prefix`.
#[track_caller]
pub fn not_ran(lines: &[String], prefix: &str) {
    let found = lines.iter().find(|line| line.starts_with(prefix));
    assert!(
        found.is_none(),
        "`{}` ran, but nothing starting with `{prefix}` should have; the commands that ran:\n{}",
        found.map_or("", String::as_str),
        command_list(lines)
    );
}

/// Runs `check` on each case and fails the test, naming every case that
/// failed and why, if any did. Use it through [`cases!`](crate::cases).
#[track_caller]
pub fn run_cases<T>(cases: Vec<(&str, T)>, check: impl Fn(T)) {
    let failures: Vec<String> = cases
        .into_iter()
        .filter_map(|(name, case)| {
            let result = catch_unwind(AssertUnwindSafe(|| check(case)));
            result.err().map(|payload| {
                let why = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| {
                        payload
                            .downcast_ref::<&str>()
                            .map(|text| (*text).to_owned())
                    })
                    .unwrap_or_default();
                format!("case {name:?}: {why}")
            })
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} case(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Fails the test unless the fake system or machine ran the command
/// `line` (program and arguments joined by spaces).
#[macro_export]
macro_rules! assert_ran {
    ($fake:expr, $line:expr $(,)?) => {
        $crate::assert::ran(&$fake.runner.lines(), $line)
    };
}

/// Fails the test if the fake system or machine ran a command that starts
/// with `prefix`.
#[macro_export]
macro_rules! assert_not_ran {
    ($fake:expr, $prefix:expr $(,)?) => {
        $crate::assert::not_ran(&$fake.runner.lines(), $prefix)
    };
}

/// Fails the test if any of the secrets reached a place secrets must not go
/// on the fake system or machine (see `FakeSystem::assert_no_leak`).
#[macro_export]
macro_rules! assert_no_secret_leaked {
    ($fake:expr, $($secret:expr),+ $(,)?) => {
        $( $fake.assert_no_leak(::core::convert::AsRef::<[u8]>::as_ref($secret)); )+
    };
}

/// Runs a table of named cases through one body, and names every case that
/// fails: `cases! { for PATTERN in { "name" => value, ... } { body } }`.
#[macro_export]
macro_rules! cases {
    (for $pattern:pat_param in { $($name:literal => $case:expr),+ $(,)? } $body:block) => {
        $crate::assert::run_cases(::std::vec![$(($name, $case)),+], |$pattern| $body)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The message of the panic `body` raises.
    fn failure(body: impl FnOnce()) -> String {
        let payload = catch_unwind(AssertUnwindSafe(body)).unwrap_err();
        payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn a_missing_command_is_reported_with_the_commands_that_ran() {
        let lines = vec!["zpool import -a".to_owned(), "zfs list".to_owned()];

        let message = failure(|| ran(&lines, "zpool export tank"));

        assert_eq!(
            message,
            "`zpool export tank` did not run; the commands that ran:\n\
             \x20 0: zpool import -a\n\
             \x20 1: zfs list\n"
        );
        ran(&lines, "zfs list");
        assert!(failure(|| ran(&[], "x")).ends_with("  (no command ran)\n"));
    }

    #[test]
    fn a_command_that_should_not_run_is_named() {
        let lines = vec!["zpool import -f tank".to_owned()];

        let message = failure(|| not_ran(&lines, "zpool import -f"));

        assert!(message.starts_with("`zpool import -f tank` ran, but nothing starting with"));
        not_ran(&lines, "zpool export");
    }

    #[test]
    fn every_failing_case_is_named() {
        let message = failure(|| {
            cases! {
                for (number, even) in {
                    "two" => (2, true),
                    "three" => (3, true),
                    "four" => (4, false),
                } {
                    assert_eq!(number % 2 == 0, even, "{number}");
                }
            }
        });

        assert!(message.starts_with("2 case(s) failed:\n"), "{message}");
        assert!(message.contains("case \"three\""));
        assert!(message.contains("case \"four\""));
        assert!(!message.contains("case \"two\""));
    }
}
