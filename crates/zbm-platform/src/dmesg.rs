//! Reading the kernel log back: all of it, as `dmesg` prints it, for saved
//! logs and reports ([`kernel_log`]), and ZFSBootMenu's messages of some
//! levels, for the recovery shell ([`read_logs`]).
//!
//! The records are read from `/dev/kmsg` (ROADMAP.md CD2), without running
//! `dmesg`. Where that cannot be read, `dmesg` is: util-linux `dmesg`
//! filters by level and time itself, and its timestamps are removed from
//! the output; when that fails (BusyBox `dmesg` has no such options), the
//! raw log is filtered by the level prefixes of its lines instead.

#![allow(
    clippy::disallowed_methods,
    reason = "the real implementation runs this program"
)]

use std::fmt::Write as _;
use std::path::Path;

use crate::System;
use crate::exec::{Command, Input, Runner, Stream};

use zbm_trace::kmsg::Message;

/// The `dmesg` command that filters by `levels` (comma-separated, such as
/// `err,warn`) and, optionally, by time.
///
/// `since` is split into words, as the unquoted `${since}` of the legacy code
/// is.
pub fn filtered_command(levels: &str, since: Option<&str>) -> Command {
    let mut command = Command::new("dmesg")
        .args(["-f", "user", "--color=never", "-l", levels])
        .stderr(Stream::Null);
    if let Some(since) = since.filter(|since| !since.is_empty()) {
        command = command.arg("--since").args(
            since
                .split([' ', '\t', '\n'])
                .filter(|word| !word.is_empty()),
        );
    }
    command
}

/// Removes the timestamp `dmesg` prints at the start of each line, as `sed
/// 's/^\[.*\]\ //'` does: everything up to the last `] ` of a line that starts
/// with `[`.
pub fn strip_timestamps(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            if line.starts_with('[')
                && let Some(end) = line.trim_end_matches('\n').rfind("] ")
            {
                return line.get(end + 2..).unwrap_or(line);
            }
            line
        })
        .collect()
}

/// The line prefixes that select each level in the raw log: BusyBox reports
/// user messages 8 levels higher than util-linux does, so both numbers are
/// accepted. An unknown level selects every line.
pub fn raw_prefixes(levels: &str) -> Vec<&'static str> {
    let mut names: Vec<&str> = levels.split(',').collect();
    // `read -a` with IFS=, does not create an empty field after a trailing comma.
    if levels.ends_with(',') {
        names.pop();
    }
    names
        .into_iter()
        .flat_map(|name| match name {
            "err" => vec!["<11>", "<3>"],
            "warn" => vec!["<12>", "<4>"],
            "notice" => vec!["<13>", "<5>"],
            "info" => vec!["<14>", "<6>"],
            "debug" => vec!["<15>", "<7>"],
            _ => vec![""],
        })
        .collect()
}

/// Filters the raw log (`dmesg -r`) by level, and prints the third and later
/// fields of ZFSBootMenu's lines, as the `grep | awk` pipeline of the legacy
/// code does. A line that holds both `ZFSBootMenu:` and `ZBM:` is printed
/// twice.
pub fn filter_raw(raw: &str, levels: &str) -> String {
    let prefixes = raw_prefixes(levels);
    let mut text = String::new();
    for line in raw.split('\n') {
        // `grep -e .` selects every non-empty line; the other patterns are prefixes.
        let selected = prefixes.iter().any(|prefix| {
            if prefix.is_empty() {
                !line.is_empty()
            } else {
                line.starts_with(prefix)
            }
        });
        if !selected {
            continue;
        }
        let fields: Vec<&str> = line.split([' ', '\t']).filter(|f| !f.is_empty()).collect();
        let mut printed = String::new();
        for field in fields.iter().skip(2) {
            printed.push_str(field);
            printed.push(' ');
        }
        for marker in ["ZFSBootMenu:", "ZBM:"] {
            if line.contains(marker) {
                text.push_str(&printed);
                text.push('\n');
            }
        }
    }
    text
}

/// Where the kernel log is read and written.
pub const KMSG: &str = "/dev/kmsg";

/// The most of the kernel log that is read: the kernel keeps at most 32
/// MiB (`log_buf_len`), and usually far less.
const KMSG_LIMIT: usize = 32 << 20;

/// The facility of messages from user space (`LOG_USER`), which those that
/// ZFSBootMenu writes to `/dev/kmsg` without one get.
const USER: u8 = 1;

/// The level names of `dmesg -l`, by level.
const LEVELS: [&str; 8] = [
    "emerg", "alert", "crit", "err", "warn", "notice", "info", "debug",
];

/// A record of the kernel log, as `/dev/kmsg` gives it:
/// `PRIORITY,SEQUENCE,MICROSECONDS,FLAGS;TEXT`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Who wrote it: 0 for the kernel, 1 for user space, and so on.
    pub facility: u8,
    /// How grave it is, 0 (`emerg`) to 7 (`debug`).
    pub level: u8,
    /// When, in microseconds since the boot.
    pub micros: u64,
    /// The text, with the kernel's `\xNN` escapes undone.
    pub text: String,
}

/// The records in `raw`, what `/dev/kmsg` gave: a line each, after which
/// lines that start with a space hold its dictionary (`KEY=value`), which
/// is left out, as are lines that are no record.
pub fn records(raw: &[u8]) -> Vec<Record> {
    String::from_utf8_lossy(raw)
        .split('\n')
        .filter(|line| !line.starts_with(' '))
        .filter_map(record)
        .collect()
}

/// The record of one line of `/dev/kmsg`.
fn record(line: &str) -> Option<Record> {
    let (prefix, text) = line.split_once(';')?;
    let mut fields = prefix.split(',');
    let priority: u16 = fields.next()?.parse().ok()?;
    let _sequence = fields.next()?;
    let micros = fields.next()?.parse().ok()?;
    Some(Record {
        facility: u8::try_from(priority >> 3).ok()?,
        level: u8::try_from(priority & 7).ok()?,
        micros,
        text: unescape(text),
    })
}

/// `text` with the kernel's escapes (`\x0a` and the like, for control
/// characters and for `\` itself) undone, as `dmesg` does.
fn unescape(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        let escaped = (byte == b'\\')
            .then(|| after.strip_prefix(b"x"))
            .flatten()
            .and_then(|hex| hex.get(..2))
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u8::from_str_radix(digits, 16).ok());
        if let Some(value) = escaped {
            bytes.push(value);
            rest = after.get(3..).unwrap_or_default();
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The records of the kernel log, read from `/dev/kmsg`.
///
/// # Errors
///
/// When `/dev/kmsg` cannot be read.
pub fn read_records(system: &System<'_>) -> std::io::Result<Vec<Record>> {
    let raw = system.files.read_available(Path::new(KMSG), KMSG_LIMIT)?;
    Ok(records(&raw))
}

/// `records` as `dmesg` prints them: `[SECONDS.MICROSECONDS] TEXT`.
pub fn format_records(records: &[Record]) -> String {
    let mut text = String::new();
    for record in records {
        let _ = writeln!(
            text,
            "[{:5}.{:06}] {}",
            record.micros / 1_000_000,
            record.micros % 1_000_000,
            record.text
        );
    }
    text
}

/// The kernel log as `dmesg` prints it, with its timestamps; empty when it
/// cannot be read. `dmesg` reads it when `/dev/kmsg` cannot be.
pub fn kernel_log(system: &System<'_>) -> String {
    if let Ok(records) = read_records(system) {
        return format_records(&records);
    }
    let command = Command::new("dmesg")
        .stdin(Input::Null)
        .stdout(Stream::Capture)
        .stderr(Stream::Null);
    system
        .runner
        .run(&command)
        .map(|output| output.stdout_text())
        .unwrap_or_default()
}

/// The levels that `levels` (comma-separated names, such as `err,warn`)
/// selects; `None` when one is not a level's name.
fn level_numbers(levels: &str) -> Option<Vec<u8>> {
    levels
        .split(',')
        .filter(|name| !name.is_empty())
        .map(|name| {
            LEVELS
                .iter()
                .position(|level| *level == name)
                .and_then(|level| u8::try_from(level).ok())
        })
        .collect()
}

/// Reads ZFSBootMenu's messages of the given levels from the kernel log,
/// as `dmesg -f user -l LEVELS` prints them without their timestamps,
/// ending with one newline.
///
/// # Errors
///
/// An error message, and nothing read, when no level is given.
pub fn read_logs(system: &System<'_>, levels: &str) -> Result<String, Message> {
    if levels.is_empty() {
        return Err(Message::error("levels is undefined"));
    }
    if let Some(wanted) = level_numbers(levels)
        && let Ok(records) = read_records(system)
    {
        let mut text = String::new();
        for record in records
            .iter()
            .filter(|record| record.facility == USER && wanted.contains(&record.level))
        {
            text.push_str(&record.text);
            text.push('\n');
        }
        return Ok(format!("{}\n", text.trim_end_matches('\n')));
    }
    read_logs_with_dmesg(system.runner, levels, None)
}

/// Reads ZFSBootMenu's messages of the given levels with the `dmesg`
/// program, as the legacy code does, also since a time.
///
/// # Errors
///
/// An error message, and nothing read, when no level is given.
pub fn read_logs_with_dmesg(
    runner: &dyn Runner,
    levels: &str,
    since: Option<&str>,
) -> Result<String, Message> {
    if levels.is_empty() {
        return Err(Message::error("levels is undefined"));
    }
    if let Ok(output) = runner.run(&filtered_command(levels, since))
        && output.status.is_success()
    {
        // The output ends with one newline, however many `dmesg` printed.
        let stdout = output.stdout_text();
        let text = format!("{}\n", stdout.trim_end_matches('\n'));
        return Ok(strip_timestamps(&text));
    }
    let raw = runner
        .run(&Command::new("dmesg").arg("-r"))
        .map(|output| output.stdout_text())
        .unwrap_or_default();
    Ok(filter_raw(&raw, levels))
}
