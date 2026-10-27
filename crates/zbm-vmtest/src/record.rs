//! The record of one test: its steps and checks in `steps.log` and on the
//! standard output, and its result in `result.json`.

use std::cell::RefCell;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};
use crate::settings::{Settings, ZFS_VERSION, ZbmRuntime};

/// The result of a test, as `result.json` holds it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Outcome {
    /// The test.
    pub test: String,
    /// Checks that passed.
    pub passed: usize,
    /// The checks that failed.
    pub failures: Vec<String>,
    /// Why parts of it did not run, if they did not.
    pub skipped: Vec<String>,
    /// How long it ran.
    pub seconds: u64,
}

impl Outcome {
    /// The result as JSON, as the Python harness wrote it.
    pub fn to_json(&self) -> String {
        let value = serde_json::json!({
            "test": self.test,
            "passed": self.passed,
            "failed": self.failures.len(),
            "failures": self.failures,
            "skipped": self.skipped,
            "seconds": self.seconds,
            "zfs": ZFS_VERSION,
        });
        serde_json::to_string_pretty(&value).unwrap_or_default() + "\n"
    }

    /// The result in `text`, a `result.json`.
    pub fn from_json(text: &str) -> Option<Outcome> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let failures = value["failures"]
            .as_array()?
            .iter()
            .filter_map(|failure| failure.as_str().map(str::to_owned))
            .collect();
        let skipped = value["skipped"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|reason| reason.as_str().map(str::to_owned))
            .collect();
        Some(Outcome {
            test: value["test"].as_str()?.to_owned(),
            passed: usize::try_from(value["passed"].as_u64()?).ok()?,
            failures,
            skipped,
            seconds: value["seconds"].as_u64().unwrap_or_default(),
        })
    }

    /// Whether every check passed.
    pub fn is_pass(&self) -> bool {
        self.failures.is_empty()
    }

    /// The verdict: `FAIL` when a check failed, `SKIP` when nothing was
    /// checked because the test did not run, `PASS` otherwise.
    pub fn verdict(&self) -> &'static str {
        if !self.is_pass() {
            "FAIL"
        } else if self.passed == 0 && !self.skipped.is_empty() {
            "SKIP"
        } else {
            "PASS"
        }
    }
}

/// What a test has recorded so far.
struct Tally {
    log: File,
    passed: usize,
    failures: Vec<String>,
    skipped: Vec<String>,
}

/// One test.
pub struct Test {
    name: String,
    dir: PathBuf,
    settings: Settings,
    start: Instant,
    tally: RefCell<Tally>,
}

impl Test {
    /// Starts the test `name`. With Rust images, the name of a test that
    /// does not choose its runtime gets `-rust`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when its directory cannot be made.
    pub fn new(settings: &Settings, name: &str) -> Result<Test> {
        let mut name = name.to_owned();
        if settings.runtime == ZbmRuntime::Rust && !name.starts_with("runtime-") {
            name.push_str("-rust");
        }
        let dir = settings.work.join("tests").join(&name);
        fs::create_dir_all(&dir).map_err(Error::io(format!("creating {}", dir.display())))?;
        let log = File::create(dir.join("steps.log")).map_err(Error::io("creating steps.log"))?;
        let test = Test {
            name,
            dir,
            settings: settings.clone(),
            start: Instant::now(),
            tally: RefCell::new(Tally {
                log,
                passed: 0,
                failures: Vec::new(),
                skipped: Vec::new(),
            }),
        };
        test.step(&format!(
            "test {}: ZBMTEST_WORK={} image={} accel={}",
            test.name,
            settings.work.display(),
            settings.image,
            settings.accel
        ));
        Ok(test)
    }

    /// The test's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The test's directory: logs, serial consoles, results.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The settings of the run.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Records a step.
    pub fn step(&self, message: &str) {
        let line = format!("[{}] {message}", clock());
        println!("{line}");
        let mut tally = self.tally.borrow_mut();
        writeln!(tally.log, "{line}").ok();
    }

    /// Records a check.
    pub fn check(&self, passed: bool, message: &str) {
        self.step(&format!(
            "{}{message}",
            if passed { "ok   - " } else { "FAIL - " }
        ));
        let mut tally = self.tally.borrow_mut();
        if passed {
            tally.passed += 1;
        } else {
            tally.failures.push(message.to_owned());
        }
    }

    /// Records a note.
    pub fn note(&self, message: &str) {
        self.step(&format!("note - {message}"));
    }

    /// Records that a part of the test, or all of it, did not run, and
    /// why: a test that checked nothing for this reason is `SKIP`, not
    /// `PASS`.
    pub fn skip(&self, reason: &str) {
        self.step(&format!("skip - {reason}"));
        self.tally.borrow_mut().skipped.push(reason.to_owned());
    }

    /// Records the lines of `text` that are not blank, indented.
    pub fn show(&self, text: &str) {
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            self.step(&format!("      {line}"));
        }
    }

    /// Ends the test: writes `result.json` and records the verdict.
    pub fn finish(&self) -> Outcome {
        let checked = {
            let tally = self.tally.borrow();
            tally.passed > 0 || !tally.failures.is_empty() || !tally.skipped.is_empty()
        };
        if !checked {
            self.check(false, "the test made a check");
        }
        let outcome = {
            let tally = self.tally.borrow();
            Outcome {
                test: self.name.clone(),
                passed: tally.passed,
                failures: tally.failures.clone(),
                skipped: tally.skipped.clone(),
                seconds: self.start.elapsed().as_secs(),
            }
        };
        fs::write(self.dir.join("result.json"), outcome.to_json()).ok();
        if outcome.verdict() == "SKIP" {
            self.step(&format!("SKIP: {}", outcome.skipped.join("; ")));
        } else if outcome.is_pass() {
            self.step("PASS");
        } else {
            let failures = crate::pyfmt::list(&outcome.failures);
            self.step(&format!("FAIL ({}): {failures}", outcome.failures.len()));
        }
        outcome
    }
}

/// The local time of day, `HH:MM:SS`, as steps show it.
pub fn clock() -> String {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    let offset = *OFFSET.get_or_init(utc_offset);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let seconds = (i64::try_from(now).unwrap_or_default() + offset).rem_euclid(86_400);
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// The offset of local time from UTC, in seconds, as `date +%z` gives it.
fn utc_offset() -> i64 {
    let output = Command::new("date").arg("+%z").output();
    let text = output
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    parse_offset(&text).unwrap_or_default()
}

/// `+0200` as seconds.
fn parse_offset(text: &str) -> Option<i64> {
    let (sign, digits) = match text.split_at_checked(1)? {
        ("+", digits) => (1, digits),
        ("-", digits) => (-1, digits),
        _ => return None,
    };
    let hours: i64 = digits.get(..2)?.parse().ok()?;
    let minutes: i64 = digits.get(2..4)?.parse().ok()?;
    Some(sign * (hours * 3600 + minutes * 60))
}

#[cfg(test)]
mod tests;
