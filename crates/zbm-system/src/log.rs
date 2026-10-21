//! Writing to the kernel log.
//!
//! [`KernelLog`] writes messages as `kmsg-log-lib.sh` does: to `/dev/kmsg` in
//! the formats of `zbm-log`, filtered by `loglevel`, with the marker files
//! `have_warnings` and `have_errors` in the state directory. It writes through
//! [`Files`], so tests see every message.

use std::panic::Location;
use std::path::{Path, PathBuf};

use zbm_log::{Level, Message, kmsg_lines, parse_level};

use crate::files::Files;

/// The kernel log device.
pub const KMSG: &str = "/dev/kmsg";

/// Writes messages to the kernel log.
pub struct KernelLog<'a> {
    files: &'a dyn Files,
    device: PathBuf,
    base: PathBuf,
    loglevel: Option<i64>,
    pid: u32,
    labelled: bool,
}

impl<'a> KernelLog<'a> {
    /// A log with the given `loglevel` (as `zbm-log` parses it) that writes to
    /// `/dev/kmsg`, leaves marker files in `base`, and names process `pid` in
    /// debugging output.
    pub fn new(files: &'a dyn Files, loglevel: &str, base: &Path, pid: u32) -> KernelLog<'a> {
        KernelLog {
            files,
            device: PathBuf::from(KMSG),
            base: base.to_owned(),
            loglevel: parse_level(loglevel),
            pid,
            labelled: false,
        }
    }

    /// The same log, writing each message of any level as one `LEVEL: text`
    /// line, as the logging stand-ins of the LUKS unit suites do; empty
    /// debugging output is left out.
    pub fn labelled(mut self) -> KernelLog<'a> {
        self.labelled = true;
        self
    }

    /// The same log, writing to another device.
    pub fn with_device(mut self, device: &Path) -> KernelLog<'a> {
        self.device = device.to_owned();
        self
    }

    /// Whether messages of `level` are written.
    pub fn enabled(&self, level: Level) -> bool {
        self.loglevel
            .is_some_and(|loglevel| loglevel >= level.number())
    }

    /// Writes a message if its level is enabled, and returns whether it was, as
    /// the legacy functions return. Debugging output names the caller. Failures
    /// to write are ignored, as `echo ... > /dev/kmsg` ignores them.
    #[track_caller]
    pub fn log(&self, message: &Message) -> bool {
        if !self.labelled && !self.enabled(message.level) {
            return false;
        }
        let marker = match message.level {
            Level::Warning => Some("have_warnings"),
            Level::Error => Some("have_errors"),
            _ => None,
        };
        if let Some(marker) = marker {
            let _ = self.files.write(&self.base.join(marker), b"");
        }
        if self.labelled {
            if message.level != Level::Debug || !message.text.is_empty() {
                let line = format!("{}\n", message.stderr_line());
                let _ = self.files.append(&self.device, line.as_bytes());
            }
            return true;
        }
        let caller = Location::caller();
        let trace = format!("{},{}", caller.file(), caller.line());
        for line in kmsg_lines(message.level, &message.text, self.pid, &trace) {
            // The kernel makes one record of each write.
            let _ = self
                .files
                .append(&self.device, format!("{line}\n").as_bytes());
        }
        true
    }

    /// Writes an error (`zerror`).
    #[track_caller]
    pub fn error(&self, text: impl Into<String>) -> bool {
        self.log(&Message::error(text))
    }

    /// Writes a warning (`zwarn`).
    #[track_caller]
    pub fn warning(&self, text: impl Into<String>) -> bool {
        self.log(&Message::warning(text))
    }

    /// Writes a notice (`znotice`).
    #[track_caller]
    pub fn notice(&self, text: impl Into<String>) -> bool {
        self.log(&Message::notice(text))
    }

    /// Writes information (`zinfo`).
    #[track_caller]
    pub fn info(&self, text: impl Into<String>) -> bool {
        self.log(&Message::info(text))
    }

    /// Writes debugging output (`zdebug`).
    #[track_caller]
    pub fn debug(&self, text: impl Into<String>) -> bool {
        self.log(&Message::debug(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::RealFiles;
    use zbm_testutil::TempDir;

    #[test]
    fn writes_records_and_markers() {
        let dir = TempDir::new("zbm-log");
        let kmsg = dir.join("kmsg");
        let log = KernelLog::new(&RealFiles, "6", dir.path(), 42).with_device(&kmsg);
        assert!(log.info("importing tank"));
        assert!(!log.debug("not shown"));
        assert!(log.warning("canmount=on set for 'tank/ROOT'"));
        assert!(log.error("unable to import a pool"));
        assert!(log.notice("note"));
        assert_eq!(
            std::fs::read_to_string(&kmsg).unwrap(),
            "<6>ZFSBootMenu: importing tank\n\
             <4>ZFSBootMenu: canmount=on set for 'tank/ROOT'\n\
             <3>ZFSBootMenu: unable to import a pool\n\
             <5>ZFSBootMenu: note\n"
        );
        assert!(dir.join("have_warnings").exists());
        assert!(dir.join("have_errors").exists());
        assert!(log.enabled(Level::Info) && !log.enabled(Level::Debug));
    }

    #[test]
    fn labelled_lines_are_written_as_the_suites_expect() {
        let dir = TempDir::new("zbm-log");
        let log_file = dir.join("log");
        let log = KernelLog::new(&RealFiles, "0", dir.path(), 42)
            .with_device(&log_file)
            .labelled();
        log.info("one");
        log.warning("two\nlines");
        log.debug("");
        log.debug("three");
        assert_eq!(
            std::fs::read_to_string(&log_file).unwrap(),
            "INFO: one\nWARN: two\nlines\nDEBUG: three\n"
        );
        // Every level is written, whatever the log level says.
        assert!(dir.join("have_warnings").exists());
    }

    #[test]
    fn debugging_output_names_the_caller() {
        let dir = TempDir::new("zbm-log");
        let kmsg = dir.join("kmsg");
        let log = KernelLog::new(&RealFiles, "7", dir.path(), 42).with_device(&kmsg);
        let line = line!() + 1;
        log.debug("one\ntwo");
        let text = std::fs::read_to_string(&kmsg).unwrap();
        assert_eq!(
            text,
            format!("<7>ZBM:[42]||one\n<7>ZBM:[42]|crates/zbm-system/src/log.rs,{line}|two\n")
        );
        assert!(!dir.join("have_errors").exists());
    }

    #[test]
    fn invalid_levels_write_nothing() {
        let dir = TempDir::new("zbm-log");
        let log = KernelLog::new(&RealFiles, "loud", dir.path(), 1).with_device(&dir.join("kmsg"));
        assert!(!log.error("lost"));
        assert!(!dir.join("kmsg").exists());
        // An unwritable device is not an error.
        let log = KernelLog::new(&RealFiles, "7", dir.path(), 1)
            .with_device(Path::new("/nonexistent/kmsg"));
        assert!(log.info("dropped"));
    }
}
