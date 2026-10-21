//! Logging of the Rust port of ZFSBootMenu.
//!
//! This is the Rust counterpart of `kmsg-log-lib.sh` and `echo-log-lib.sh`. In
//! the boot image, messages go to the kernel log (`/dev/kmsg`) as
//! `<N>ZFSBootMenu: message`, and warnings and errors leave marker files in the
//! base directory; host tools write `LEVEL: message` lines to standard error. A
//! message is written when the log level is at least the message's level, and
//! no message is written when the log level is not an integer, as with the
//! legacy `[ "${loglevel}" -ge N ]` tests. [`read`] reads the kernel log back,
//! as `print_kmsg_logs` does.

use std::fs::OpenOptions;
use std::io::Write;
use std::panic::Location;
use std::path::PathBuf;

pub mod read;

/// The level of a message, with the numbers of the kernel log.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Level {
    /// An error (`zerror`, level 3).
    Error,
    /// A warning (`zwarn`, level 4).
    Warning,
    /// A notice (`znotice`, level 5).
    Notice,
    /// Information (`zinfo`, level 6).
    Info,
    /// Debugging output (`zdebug`, level 7).
    Debug,
}

impl Level {
    /// The kernel log level.
    pub fn number(self) -> i64 {
        match self {
            Level::Error => 3,
            Level::Warning => 4,
            Level::Notice => 5,
            Level::Info => 6,
            Level::Debug => 7,
        }
    }

    /// The label `echo-log-lib.sh` prints before a message on standard error.
    pub fn label(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warning => "WARN",
            Level::Notice => "NOTICE",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
        }
    }
}

/// A message to log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message {
    /// The level.
    pub level: Level,
    /// The text.
    pub text: String,
}

impl Message {
    /// A message with the given level.
    pub fn new(level: Level, text: impl Into<String>) -> Message {
        Message {
            level,
            text: text.into(),
        }
    }

    /// An error.
    pub fn error(text: impl Into<String>) -> Message {
        Message::new(Level::Error, text)
    }

    /// A warning.
    pub fn warning(text: impl Into<String>) -> Message {
        Message::new(Level::Warning, text)
    }

    /// A notice.
    pub fn notice(text: impl Into<String>) -> Message {
        Message::new(Level::Notice, text)
    }

    /// Information.
    pub fn info(text: impl Into<String>) -> Message {
        Message::new(Level::Info, text)
    }

    /// Debugging output.
    pub fn debug(text: impl Into<String>) -> Message {
        Message::new(Level::Debug, text)
    }

    /// The message as `echo-log-lib.sh` prints it: `LEVEL: text`.
    pub fn stderr_line(&self) -> String {
        format!("{}: {}", self.level.label(), self.text)
    }
}

/// The log level as the legacy code reads it: an integer operand of `test`, or
/// `None` when it is not one, in which case nothing is logged.
pub fn parse_level(loglevel: &str) -> Option<i64> {
    zbm_core::shell::test_integer(loglevel)
}

/// The kernel log lines of a message, as `kmsg-log-lib.sh` writes them.
///
/// Debugging output is written line by line: every line starts with
/// `<7>ZBM:[pid]|`, and the last line carries the call site between the bars
/// (`<7>ZBM:[pid]|trace|line`). Other levels are written as `<N>ZFSBootMenu:
/// text`.
pub fn kmsg_lines(level: Level, text: &str, pid: u32, trace: &str) -> Vec<String> {
    match level {
        Level::Debug => {
            let lines: Vec<&str> = text.split('\n').collect();
            let last = lines.len() - 1;
            lines
                .iter()
                .enumerate()
                .map(|(index, line)| {
                    let trace = if index == last { trace } else { "" };
                    format!("<7>ZBM:[{pid}]|{trace}|{line}")
                })
                .collect()
        }
        _ => vec![format!("<{}>ZFSBootMenu: {text}", level.number())],
    }
}

/// Where a [`Logger`] writes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Sink {
    /// The kernel log, as in the boot image: messages are written to `device`, and
    /// warnings and errors create `have_warnings` and `have_errors` in `base`.
    Kmsg {
        /// The kernel log device, normally `/dev/kmsg`.
        device: PathBuf,
        /// The base directory, normally `/zfsbootmenu`.
        base: String,
    },
    /// Standard error, as host tools write.
    Stderr,
}

/// Writes messages whose level the log level allows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Logger {
    loglevel: Option<i64>,
    sink: Sink,
}

impl Logger {
    /// A logger with the given log level (see [`parse_level`]) and sink.
    pub fn new(loglevel: &str, sink: Sink) -> Logger {
        Logger {
            loglevel: parse_level(loglevel),
            sink,
        }
    }

    /// A logger for the boot image: `/dev/kmsg`, with marker files in `base`.
    pub fn kmsg(loglevel: &str, base: &str) -> Logger {
        Logger::new(
            loglevel,
            Sink::Kmsg {
                device: PathBuf::from("/dev/kmsg"),
                base: base.to_owned(),
            },
        )
    }

    /// A logger for host tools, writing to standard error.
    pub fn stderr(loglevel: &str) -> Logger {
        Logger::new(loglevel, Sink::Stderr)
    }

    /// Whether messages of `level` are written.
    pub fn enabled(&self, level: Level) -> bool {
        self.loglevel
            .is_some_and(|loglevel| loglevel >= level.number())
    }

    /// Writes a message if its level is enabled, and returns whether it was.
    /// Debugging output records the caller.
    #[track_caller]
    pub fn log(&self, message: &Message) -> bool {
        if !self.enabled(message.level) {
            return false;
        }
        match &self.sink {
            Sink::Stderr => {
                // Like `echo ... >&2`, a failure to write is not reported.
                let line = format!("{}\n", message.stderr_line());
                let _ = std::io::stderr().write_all(line.as_bytes());
            }
            Sink::Kmsg { device, base } => {
                let marker = match message.level {
                    Level::Warning => Some("have_warnings"),
                    Level::Error => Some("have_errors"),
                    _ => None,
                };
                if let Some(marker) = marker {
                    // `: > "${BASE}/have_errors"`
                    let _ = std::fs::write(format!("{base}/{marker}"), b"");
                }
                let caller = Location::caller();
                let trace = format!("{},{}", caller.file(), caller.line());
                for line in kmsg_lines(message.level, &message.text, std::process::id(), &trace) {
                    // The kernel makes one record of each write, so every line is written at once.
                    let record = format!("{line}\n");
                    if let Ok(mut kmsg) = OpenOptions::new().append(true).open(device) {
                        let _ = kmsg.write_all(record.as_bytes());
                    }
                }
            }
        }
        true
    }

    /// Writes all messages whose level is enabled, in order.
    #[track_caller]
    pub fn log_all(&self, messages: &[Message]) {
        for message in messages {
            self.log(message);
        }
    }

    /// Writes an error.
    #[track_caller]
    pub fn error(&self, text: impl Into<String>) -> bool {
        self.log(&Message::error(text))
    }

    /// Writes a warning.
    #[track_caller]
    pub fn warning(&self, text: impl Into<String>) -> bool {
        self.log(&Message::warning(text))
    }

    /// Writes a notice.
    #[track_caller]
    pub fn notice(&self, text: impl Into<String>) -> bool {
        self.log(&Message::notice(text))
    }

    /// Writes information.
    #[track_caller]
    pub fn info(&self, text: impl Into<String>) -> bool {
        self.log(&Message::info(text))
    }

    /// Writes debugging output.
    #[track_caller]
    pub fn debug(&self, text: impl Into<String>) -> bool {
        self.log(&Message::debug(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testutil::TempDir;

    #[test]
    fn levels_match_the_kernel_log() {
        let numbers: Vec<i64> = [
            Level::Error,
            Level::Warning,
            Level::Notice,
            Level::Info,
            Level::Debug,
        ]
        .iter()
        .map(|level| level.number())
        .collect();
        assert_eq!(numbers, [3, 4, 5, 6, 7]);
        assert_eq!(Message::warning("w").stderr_line(), "WARN: w");
    }

    #[test]
    fn log_level_decides_what_is_written() {
        let logger = Logger::stderr("4");
        assert!(logger.enabled(Level::Error));
        assert!(logger.enabled(Level::Warning));
        assert!(!logger.enabled(Level::Notice));
        assert!(Logger::stderr("+7").enabled(Level::Debug));
        // A log level that is not an integer disables everything, as `[ x -ge 3 ]`
        // fails.
        assert!(!Logger::stderr("high").enabled(Level::Error));
        assert!(!Logger::stderr("").enabled(Level::Error));
    }

    #[test]
    fn kernel_log_lines_follow_kmsg_log_lib() {
        assert_eq!(
            kmsg_lines(Level::Info, "hello", 7, "t"),
            ["<6>ZFSBootMenu: hello"]
        );
        assert_eq!(
            kmsg_lines(Level::Error, "a\nb", 7, "t"),
            ["<3>ZFSBootMenu: a\nb"]
        );
        assert_eq!(
            kmsg_lines(Level::Debug, "one\ntwo", 7, "f,1"),
            ["<7>ZBM:[7]||one", "<7>ZBM:[7]|f,1|two"]
        );
        assert_eq!(kmsg_lines(Level::Debug, "", 7, "f,1"), ["<7>ZBM:[7]|f,1|"]);
        assert_eq!(
            kmsg_lines(Level::Debug, "x\n", 7, "f,1"),
            ["<7>ZBM:[7]||x", "<7>ZBM:[7]|f,1|"]
        );
    }

    #[test]
    fn kernel_log_sink_writes_lines_and_markers() {
        let dir = TempDir::new("zbm-log");
        let device = dir.join("kmsg");
        let base = dir.path().to_string_lossy().into_owned();
        let logger = Logger::new(
            "7",
            Sink::Kmsg {
                device: device.clone(),
                base: base.clone(),
            },
        );
        std::fs::write(&device, b"").unwrap();
        assert!(logger.warning("careful"));
        assert!(logger.debug("detail"));
        assert!(logger.error("broken"));
        let written = std::fs::read_to_string(&device).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines[0], "<4>ZFSBootMenu: careful");
        assert!(lines[1].starts_with(&format!("<7>ZBM:[{}]|", std::process::id())));
        assert!(lines[1].ends_with("|detail"));
        assert!(lines[1].contains("lib.rs,"));
        assert_eq!(lines[2], "<3>ZFSBootMenu: broken");
        assert!(dir.join("have_warnings").is_file());
        assert!(dir.join("have_errors").is_file());

        let quiet = Logger::new(
            "3",
            Sink::Kmsg {
                device: device.clone(),
                base,
            },
        );
        assert!(!quiet.info("hidden"));
        assert_eq!(std::fs::read_to_string(&device).unwrap(), written);
    }
}
