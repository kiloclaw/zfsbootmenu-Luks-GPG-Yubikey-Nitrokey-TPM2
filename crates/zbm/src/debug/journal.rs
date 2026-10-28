//! `zbm debug journal` and `zbm debug commands`: the runtime journal
//! (`journal.jsonl` in the state directory) for people, in place of the
//! legacy `zlogtail` and `ztrace`.
//!
//! - `journal [--level LEVEL]` prints each record at `LEVEL` or above
//!   (`error`, `warn`, `info`, `debug`, `trace`; all by default) on one line:
//!   the program's process ID, the level, the target, the message, its
//!   fields, and the spans it happened in, outermost first.
//! - `commands` prints each program the runtime ran, with its exit status
//!   and how long it took, and a summary.
//!
//! Secrets never reach the journal ([`zbm_trace::REDACTED`]), so neither
//! report can show one.

use std::path::Path;
use zbm_types::Printable;

use serde_json::Value;
use zbm_platform::Files;

use crate::Io;

/// The journal's file in the state directory.
pub use zbm_platform::log::JOURNAL;

/// The levels, from the most severe.
const LEVELS: [&str; 5] = ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"];

/// The rank of `level` among [`LEVELS`] (0 for errors); unknown levels
/// count as the least severe.
fn rank(level: &str) -> usize {
    LEVELS
        .iter()
        .position(|known| known.eq_ignore_ascii_case(level))
        .unwrap_or(LEVELS.len())
}

/// The records of the journal in `base`, in the order they were written;
/// lines that are not JSON are left out.
pub fn records(files: &dyn Files, base: &Path) -> Vec<Value> {
    files
        .read_to_string(&base.join(JOURNAL))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// A JSON value as text: strings without quotes.
fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// `key=value` for each member of the object `value`, but `skip`.
fn pairs(value: Option<&Value>, skip: &[&str]) -> Vec<String> {
    value
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter(|(key, _)| !skip.contains(&key.as_str()))
                .map(|(key, value)| format!("{key}={}", text(value)))
                .collect()
        })
        .unwrap_or_default()
}

/// One record on one line.
pub fn line(record: &Value) -> String {
    let field = |key: &str| record.get(key).map(text).unwrap_or_default();
    let mut line = format!(
        "{:>6} {:<5} {}: {}",
        field("pid"),
        field("level"),
        field("target"),
        field("message")
    );
    for pair in pairs(record.get("fields"), &[]) {
        line.push(' ');
        line.push_str(&pair);
    }
    let spans: Vec<String> = record
        .get("spans")
        .and_then(Value::as_array)
        .map(|spans| {
            spans
                .iter()
                .map(|span| {
                    let name = span.get("span").map(text).unwrap_or_default();
                    let fields = pairs(Some(span), &["span"]);
                    if fields.is_empty() {
                        name
                    } else {
                        format!("{name}{{{}}}", fields.join(" "))
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    if !spans.is_empty() {
        line.push_str(" [");
        line.push_str(&spans.join(" > "));
        line.push(']');
    }
    line
}

/// `zbm debug journal [--level LEVEL]`.
pub fn journal(files: &dyn Files, base: &Path, args: &[&str], io: &mut Io) -> Option<i32> {
    let limit = match args {
        [] => LEVELS.len(),
        ["--level", level] if rank(level) < LEVELS.len() => rank(level),
        _ => return None,
    };
    let records = records(files, base);
    if records.is_empty() {
        let _ = writeln!(io.stderr, "zbm debug: no journal in {}", base.display());
        return Some(1);
    }
    for record in records
        .iter()
        .filter(|record| rank(record.get("level").and_then(Value::as_str).unwrap_or("")) <= limit)
    {
        // The journal holds names and output from outside ZFSBootMenu.
        let _ = writeln!(io.stdout, "{}", Printable(&line(record)));
    }
    Some(0)
}

/// A program the runtime ran, as the journal recorded it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Ran {
    /// The command line, as `zbm` shows commands.
    pub line: String,
    /// Its exit status, or `None` when it could not start.
    pub status: Option<i64>,
    /// How long it took, in milliseconds.
    pub duration_ms: u64,
}

/// The programs in `records`, from the events of the traced runner.
pub fn commands(records: &[Value]) -> Vec<Ran> {
    records
        .iter()
        .filter_map(|record| {
            let message = record.get("message")?.as_str()?;
            if message != "command ended" && message != "command could not start" {
                return None;
            }
            let span = record
                .get("spans")?
                .as_array()?
                .iter()
                .rev()
                .find(|span| span.get("span").and_then(Value::as_str) == Some("command"))?;
            let fields = record.get("fields");
            let number = |key: &str| fields.and_then(|fields| fields.get(key));
            Some(Ran {
                line: span.get("line").map(text).unwrap_or_default(),
                status: number("status").and_then(Value::as_i64),
                duration_ms: number("duration_ms").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect()
}

/// `zbm debug commands`.
pub fn print_commands(files: &dyn Files, base: &Path, io: &mut Io) -> i32 {
    let ran = commands(&records(files, base));
    for command in &ran {
        let status = command
            .status
            .map_or_else(|| "-".to_owned(), |status| status.to_string());
        let _ = writeln!(
            io.stdout,
            "{status:>4} {:>7} ms  {}",
            command.duration_ms,
            Printable(&command.line)
        );
    }
    let failed = ran
        .iter()
        .filter(|command| command.status != Some(0))
        .count();
    let total: u64 = ran.iter().map(|command| command.duration_ms).sum();
    let _ = writeln!(
        io.stdout,
        "{} commands, {failed} failed or did not start, {total} ms in all",
        ran.len()
    );
    0
}

#[cfg(test)]
mod tests;
