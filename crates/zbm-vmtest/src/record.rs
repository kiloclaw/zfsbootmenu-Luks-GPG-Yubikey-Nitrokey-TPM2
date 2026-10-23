//! The record of one test: its steps and checks in `steps.log` and on the
//! standard output, and its result in `result.json`.

use std::cell::RefCell;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;

use crate::error::{Error, Result};
use crate::settings::{Settings, ZFS_VERSION, ZbmRuntime};
use crate::text::clean;

/// The summary shadow mode prints at each boot.
const SHADOW_SUMMARY: &str = r"ZBM-SHADOW: (\d+) differences in \d+ decisions(?: \(([^)\n]*)\))?";

/// The result of a test, as `result.json` holds it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Outcome {
    /// The test.
    pub test: String,
    /// Checks that passed.
    pub passed: usize,
    /// The checks that failed.
    pub failures: Vec<String>,
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
        Some(Outcome {
            test: value["test"].as_str()?.to_owned(),
            passed: usize::try_from(value["passed"].as_u64()?).ok()?,
            failures,
            seconds: value["seconds"].as_u64().unwrap_or_default(),
        })
    }

    /// Whether every check passed.
    pub fn is_pass(&self) -> bool {
        self.failures.is_empty()
    }
}

/// What a test has recorded so far.
struct Tally {
    log: File,
    passed: usize,
    failures: Vec<String>,
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

    /// Records a check; returns whether it passed.
    pub fn check(&self, passed: bool, message: &str) -> bool {
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
        passed
    }

    /// Records a note.
    pub fn note(&self, message: &str) {
        self.step(&format!("note - {message}"));
    }

    /// Records the lines of `text` that are not blank, indented.
    pub fn show(&self, text: &str) {
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            self.step(&format!("      {line}"));
        }
    }

    /// Checks the shadow mode summaries on the serial consoles of the test:
    /// the Rust code decided as the legacy runtime did on every boot.
    fn shadow(&self) {
        let Some(summary) = shadow_summary(&self.dir) else {
            return;
        };
        if !summary.lines.is_empty() {
            self.show(&summary.lines.join("\n"));
        }
        let compared = if summary.decisions.is_empty() {
            "nothing".to_owned()
        } else {
            summary.decisions.join(", ")
        };
        self.check(
            summary.differences == 0,
            &format!(
                "shadow mode: {} differences in {} boots (compared: {compared})",
                summary.differences, summary.boots
            ),
        );
    }

    /// Ends the test: checks shadow mode, writes `result.json` and records
    /// the verdict.
    pub fn finish(&self) -> Outcome {
        self.shadow();
        let outcome = {
            let tally = self.tally.borrow();
            Outcome {
                test: self.name.clone(),
                passed: tally.passed,
                failures: tally.failures.clone(),
                seconds: self.start.elapsed().as_secs(),
            }
        };
        fs::write(self.dir.join("result.json"), outcome.to_json()).ok();
        if outcome.is_pass() {
            self.step("PASS");
        } else {
            let failures = crate::pyfmt::list(&outcome.failures);
            self.step(&format!("FAIL ({}): {failures}", outcome.failures.len()));
        }
        outcome
    }
}

/// The shadow mode reports on the serial consoles of a test.
#[derive(Debug, Default, Eq, PartialEq)]
struct ShadowSummary {
    boots: usize,
    differences: u64,
    /// The decisions compared, each once, in order.
    decisions: Vec<String>,
    /// The other `ZBM-SHADOW:` lines, each once: the differences.
    lines: Vec<String>,
}

/// The shadow mode reports in the `serial-*.log` files of `dir`, if any.
fn shadow_summary(dir: &Path) -> Option<ShadowSummary> {
    let mut logs: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("serial-") && name.ends_with(".log"))
        })
        .collect();
    logs.sort();
    let texts: Vec<String> = logs
        .iter()
        .filter_map(|log| fs::read(log).ok())
        .map(|bytes| clean(&String::from_utf8_lossy(&bytes)))
        .collect();
    let summary = summarize_shadow(&texts);
    (summary.boots > 0).then_some(summary)
}

/// The shadow mode reports in the console texts `texts`.
fn summarize_shadow(texts: &[String]) -> ShadowSummary {
    static PATTERNS: OnceLock<(Regex, Regex)> = OnceLock::new();
    let (summary_re, line_re) = PATTERNS.get_or_init(|| {
        let summary = Regex::new(SHADOW_SUMMARY).unwrap_or_else(|_| unreachable!());
        let line = Regex::new(r"ZBM-SHADOW: [^\n]*").unwrap_or_else(|_| unreachable!());
        (summary, line)
    });
    let mut summary = ShadowSummary::default();
    for text in texts {
        for captures in summary_re.captures_iter(text) {
            summary.boots += 1;
            summary.differences += captures[1].parse::<u64>().unwrap_or_default();
            let decisions = captures.get(2).map_or("", |group| group.as_str());
            for decision in decisions
                .split(", ")
                .filter(|decision| !decision.is_empty())
            {
                if !summary.decisions.iter().any(|known| known == decision) {
                    summary.decisions.push(decision.to_owned());
                }
            }
        }
        for line in line_re.find_iter(text).map(|found| found.as_str()) {
            let is_summary = summary_re
                .find(line.trim())
                .is_some_and(|found| found.start() == 0);
            if !is_summary && !summary.lines.iter().any(|known| known == line) {
                summary.lines.push(line.to_owned());
            }
        }
    }
    summary
}

/// The local time of day, `HH:MM:SS`.
fn clock() -> String {
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
