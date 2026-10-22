//! Golden files of the legacy code's results, so that parity tests run
//! without Bash, Perl or the legacy tree.
//!
//! A parity test gives the same input to a legacy function and to its Rust
//! counterpart and compares the results. The legacy half runs inside
//! [`Parity::legacy`]: when recording, the closure runs and its result is
//! written to a golden file below `tests/golden/legacy/`; in a normal test run
//! the closure does not run and the recorded result is read back instead.
//!
//! ```no_run
//! use zbm_testkit::legacy::Bash;
//! use zbm_testkit::parity;
//!
//! let mut parity = parity!("doubles");
//! for n in [0, 21] {
//!     let legacy: String = parity.legacy(format!("n={n}"), || {
//!         let output = Bash::new().call("echo", &[&(2 * n).to_string()], b"");
//!         output.stdout_text()
//!     });
//!     assert_eq!(legacy, format!("{}\n", 2 * n));
//! }
//! ```
//!
//! [`Mode::current`] reads `ZBM_PARITY`:
//!
//! - `replay` (the default): read the golden files; the legacy code never runs,
//!   and [`legacy`](crate::legacy) refuses to run it.
//! - `record`: run the legacy code and write the golden files
//!   (`cargo xtask record-parity`).
//! - `check`: run the legacy code and fail where it no longer gives the
//!   recorded result (`cargo xtask record-parity --check`).
//! - `live`: run the legacy code and ignore the golden files. This is the
//!   default when `ZBM_TEST_SEED` is set, since other seeds generate inputs
//!   that were never recorded.
//!
//! Each case is recorded with a description of its input. When a test's
//! inputs change, replaying fails and names the case, and the golden file must
//! be recorded again.

use std::cell::Cell;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A [`Parity`] for the test `name` of the calling module.
///
/// ```
/// let parity = zbm_testkit::parity!("a_test");
/// assert!(parity.path().ends_with("a_test.json"));
/// ```
#[macro_export]
macro_rules! parity {
    ($test:expr) => {
        $crate::parity::Parity::new(env!("CARGO_PKG_NAME"), module_path!(), $test)
    };
}

/// What a parity test does with the legacy code and the golden files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Read the results from the golden files.
    Replay,
    /// Run the legacy code and write its results to the golden files.
    Record,
    /// Run the legacy code and compare its results with the golden files.
    Check,
    /// Run the legacy code and leave the golden files alone.
    Live,
}

impl Mode {
    /// The mode `ZBM_PARITY` and `ZBM_TEST_SEED` select.
    ///
    /// # Panics
    ///
    /// When `ZBM_PARITY` has another value.
    pub fn current() -> Mode {
        let parity = std::env::var("ZBM_PARITY").ok();
        Mode::select(
            parity.as_deref(),
            std::env::var_os("ZBM_TEST_SEED").is_some(),
        )
    }

    /// The mode for the value of `ZBM_PARITY`, if set, and whether
    /// `ZBM_TEST_SEED` is set.
    ///
    /// # Panics
    ///
    /// When `parity` is not the name of a mode.
    pub fn select(parity: Option<&str>, seeded: bool) -> Mode {
        match parity {
            Some("replay") => Mode::Replay,
            Some("record") => Mode::Record,
            Some("check") => Mode::Check,
            Some("live") => Mode::Live,
            Some(other) => panic!("ZBM_PARITY={other}: use replay, record, check or live"),
            None if seeded => Mode::Live,
            None => Mode::Replay,
        }
    }

    /// Whether the legacy code runs in this mode.
    pub fn needs_legacy(self) -> bool {
        self != Mode::Replay
    }
}

thread_local! {
    /// Whether this thread is inside [`Parity::legacy`].
    static RECORDING: Cell<bool> = const { Cell::new(false) };
}

/// Panics unless the caller runs inside [`Parity::legacy`] in a mode that
/// runs the legacy code. The helpers of [`legacy`](crate::legacy) call it
/// before they touch the legacy tree.
///
/// # Panics
///
/// Outside a recording.
pub fn assert_recording() {
    assert!(
        RECORDING.with(Cell::get),
        "legacy code runs only inside Parity::legacy, when ZBM_PARITY is record, check or live"
    );
}

/// One recorded case: the input as the test describes it, and the result.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Case {
    /// The description of the input.
    input: String,
    /// The legacy code's result.
    result: Value,
}

/// A golden file.
#[derive(Debug, Deserialize, Serialize)]
struct Golden {
    /// What wrote the file.
    about: String,
    /// The cases, in the order the test met them.
    cases: Vec<Case>,
}

/// The text of [`Golden::about`].
const ABOUT: &str =
    "Results of the legacy code, recorded by `cargo xtask record-parity`; do not edit";

/// The recorded legacy results of one test.
#[derive(Debug)]
pub struct Parity {
    /// The golden file.
    path: PathBuf,
    /// What happens to the legacy code and the file.
    mode: Mode,
    /// The cases of the file (replay and check), read when first needed:
    /// a test that skips needs no file.
    recorded: Option<Vec<Case>>,
    /// The cases met so far.
    met: Vec<Case>,
}

impl Parity {
    /// The recordings of the test `test` in `module` of the crate
    /// `package`, in `tests/golden/legacy/<package>/<module path>/<test>.json`.
    /// The module path leaves out the library's name and `tests` modules; an
    /// integration test keeps the name of its file.
    ///
    pub fn new(package: &str, module: &str, test: &str) -> Parity {
        let mut path = crate::repo_root().join("tests/golden/legacy").join(package);
        // The library's own name repeats the package; an integration test's
        // crate name says which test file it is.
        let library = package.replace('-', "_");
        for segment in module
            .split("::")
            .enumerate()
            .filter(|(index, segment)| !(*index == 0 && *segment == library))
            .map(|(_, segment)| segment)
            .filter(|segment| *segment != "tests")
        {
            path.push(segment);
        }
        path.push(format!("{test}.json"));
        Parity {
            path,
            mode: Mode::current(),
            recorded: None,
            met: Vec::new(),
        }
    }

    /// The golden file.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// The mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The legacy result for `input`: `run`'s result when the mode runs the
    /// legacy code, the recorded one otherwise.
    ///
    /// `input` describes the input of the case, so that a changed input is
    /// found: it must not contain anything that changes from run to run, such
    /// as the path of a temporary directory.
    ///
    /// # Panics
    ///
    /// When replaying and the golden file cannot be read, the case was not
    /// recorded or its input changed, and when checking and the legacy code
    /// gives another result.
    pub fn legacy<T, F>(&mut self, input: impl Into<String>, run: F) -> T
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> T,
    {
        let input = input.into();
        let index = self.met.len();
        let recorded = self.recorded().get(index).cloned();
        if let Some(case) = &recorded {
            assert!(
                case.input == input,
                "{}: case {index} was recorded for input\n  {}\n\
                 but the test now gives\n  {input}\n{RECORD_AGAIN}",
                self.path.display(),
                case.input,
            );
        }
        let result = if self.mode.needs_legacy() {
            let value = run_recording(run);
            serde_json::to_value(&value).expect("the legacy result can be written as JSON")
        } else {
            let Some(case) = recorded.clone() else {
                panic!(
                    "{}: no recording of case {index} (input {input}). {RECORD_AGAIN}",
                    self.path.display()
                );
            };
            case.result
        };
        if self.mode == Mode::Check
            && let Some(case) = &recorded
        {
            assert!(
                case.result == result,
                "{}: the legacy code no longer gives the recorded result for input {input}\n\
                 recorded: {}\nnow: {result}\n{RECORD_AGAIN}",
                self.path.display(),
                case.result
            );
        }
        self.met.push(Case {
            input,
            result: result.clone(),
        });
        serde_json::from_value(result).expect("the recorded result has the type the test expects")
    }

    /// The recorded cases; none when recording or running live.
    fn recorded(&mut self) -> &[Case] {
        let (path, mode) = (&self.path, self.mode);
        self.recorded.get_or_insert_with(|| match mode {
            Mode::Replay | Mode::Check => read(path),
            Mode::Record | Mode::Live => Vec::new(),
        })
    }
}

/// What to do when a recording is out of date.
const RECORD_AGAIN: &str = "Record the legacy results again with `cargo xtask record-parity`.";

/// Runs `run` with the legacy helpers allowed.
fn run_recording<T>(run: impl FnOnce() -> T) -> T {
    /// Clears the flag again, also when `run` panics.
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            RECORDING.with(|flag| flag.set(false));
        }
    }
    RECORDING.with(|flag| flag.set(true));
    let _reset = Reset;
    run()
}

/// The cases of the golden file at `path`.
fn read(path: &std::path::Path) -> Vec<Case> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => panic!("{}: {error}. {RECORD_AGAIN}", path.display()),
    };
    let golden: Golden = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{}: {error}. {RECORD_AGAIN}", path.display()));
    golden.cases
}

impl Drop for Parity {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        match self.mode {
            Mode::Record if !self.met.is_empty() => {
                let golden = Golden {
                    about: ABOUT.to_owned(),
                    cases: std::mem::take(&mut self.met),
                };
                let mut text = serde_json::to_string_pretty(&golden).expect("JSON");
                text.push('\n');
                if let Some(parent) = self.path.parent() {
                    std::fs::create_dir_all(parent).expect("the golden directory can be made");
                }
                std::fs::write(&self.path, text).expect("the golden file can be written");
            }
            Mode::Replay | Mode::Check if !self.met.is_empty() => {
                let recorded = self.recorded().len();
                assert!(
                    self.met.len() >= recorded,
                    "{}: {recorded} cases were recorded but the test met only {}. {RECORD_AGAIN}",
                    self.path.display(),
                    self.met.len()
                );
            }
            Mode::Replay | Mode::Check | Mode::Record | Mode::Live => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parity(mode: Mode, recorded: Vec<Case>) -> Parity {
        Parity {
            path: PathBuf::from("/golden/t.json"),
            mode,
            recorded: Some(recorded),
            met: Vec::new(),
        }
    }

    fn case(input: &str, result: Value) -> Case {
        Case {
            input: input.to_owned(),
            result,
        }
    }

    #[test]
    fn replay_returns_the_recorded_result_without_running_the_legacy_code() {
        let mut parity = parity(Mode::Replay, vec![case("a", Value::from(7))]);

        let result: u32 = parity.legacy("a", || unreachable!("the legacy code ran"));

        assert_eq!(result, 7);
    }

    #[test]
    #[should_panic(expected = "case 0 was recorded for input\n  a\nbut the test now gives\n  b")]
    fn replay_names_a_changed_input() {
        let mut parity = parity(Mode::Replay, vec![case("a", Value::from(7))]);

        let _: u32 = parity.legacy("b", || 7);
    }

    #[test]
    #[should_panic(expected = "no recording of case 0 (input a)")]
    fn replay_fails_without_a_recording() {
        let mut parity = parity(Mode::Replay, Vec::new());

        let _: u32 = parity.legacy("a", || 7);
    }

    #[test]
    #[should_panic(expected = "the legacy code no longer gives the recorded result")]
    fn check_fails_when_the_legacy_result_changed() {
        let mut parity = parity(Mode::Check, vec![case("a", Value::from(7))]);

        let _: u32 = parity.legacy("a", || 8);
    }

    #[test]
    fn the_legacy_helpers_run_only_inside_a_recording() {
        let mut parity = parity(Mode::Live, Vec::new());

        let inside = parity.legacy("a", || std::panic::catch_unwind(assert_recording).is_ok());
        let outside = std::panic::catch_unwind(assert_recording).is_ok();

        assert!(inside);
        assert!(!outside);
    }

    #[test]
    fn the_mode_follows_zbm_parity_and_a_seed_means_live() {
        assert_eq!(Mode::select(None, false), Mode::Replay);
        assert_eq!(Mode::select(None, true), Mode::Live);
        assert_eq!(Mode::select(Some("record"), true), Mode::Record);
        assert_eq!(Mode::select(Some("check"), false), Mode::Check);
        assert_eq!(Mode::select(Some("live"), false), Mode::Live);
        assert_eq!(Mode::select(Some("replay"), true), Mode::Replay);
        assert!(std::panic::catch_unwind(|| Mode::select(Some("x"), false)).is_err());
        assert!(!Mode::Replay.needs_legacy() && Mode::Check.needs_legacy());
    }

    #[test]
    fn a_recording_is_written_and_replayed() {
        let dir = crate::TempDir::new("zbm-parity");
        let path = dir.join("a/t.json");
        let mut recording = Parity {
            path: path.clone(),
            mode: Mode::Record,
            recorded: None,
            met: Vec::new(),
        };
        let recorded: Vec<u8> = recording.legacy("first", || vec![1, 2]);
        drop(recording);
        assert_eq!(recorded, [1, 2]);

        let mut replay = Parity {
            path,
            mode: Mode::Replay,
            recorded: None,
            met: Vec::new(),
        };
        let replayed: Vec<u8> = replay.legacy("first", || unreachable!("the legacy code ran"));

        assert_eq!(replayed, [1, 2]);
    }

    #[test]
    fn check_returns_what_the_legacy_code_gives_when_it_matches() {
        let mut parity = parity(Mode::Check, vec![case("a", Value::from(7))]);

        let result: u32 = parity.legacy("a", || 7);

        assert_eq!(result, 7);
    }

    #[test]
    fn live_runs_the_legacy_code_and_reads_nothing() {
        let mut parity = Parity {
            path: PathBuf::from("/nonexistent/t.json"),
            mode: Mode::Live,
            recorded: None,
            met: Vec::new(),
        };

        let result: String = parity.legacy("a", || "ran".to_owned());

        assert_eq!(result, "ran");
        assert_eq!(parity.mode(), Mode::Live);
    }

    #[test]
    fn the_mode_comes_from_the_environment() {
        let mode = Mode::current();

        assert!(matches!(
            mode,
            Mode::Replay | Mode::Record | Mode::Check | Mode::Live
        ));
    }

    #[test]
    #[should_panic(expected = "2 cases were recorded but the test met only 1")]
    fn a_case_left_out_fails() {
        let mut parity = parity(
            Mode::Replay,
            vec![case("a", Value::from(1)), case("b", Value::from(2))],
        );

        let _: u32 = parity.legacy("a", || 1);
    }

    #[test]
    fn the_golden_file_is_named_after_the_crate_module_and_test() {
        let parity = Parity::new("zbm-cmdline", "zbm_cmdline::args::tests", "lookups");

        assert!(
            parity
                .path()
                .ends_with("tests/golden/legacy/zbm-cmdline/args/lookups.json")
        );
        let integration = Parity::new("zbm-kcl", "parity", "edits");
        assert!(
            integration
                .path()
                .ends_with("tests/golden/legacy/zbm-kcl/parity/edits.json")
        );
    }
}
