//! Reading ZFSBootMenu's messages back from the kernel log, for the log
//! viewer and the recovery shell.
//!
//! util-linux `dmesg` filters by level and time itself; its timestamps are
//! removed from the output. When that fails (BusyBox `dmesg` has no such
//! options), the raw log is filtered by the level prefixes of its lines
//! instead.

use crate::exec::{Command, Runner, Stream};

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

/// Reads ZFSBootMenu's messages of the given levels from the kernel log.
///
/// # Errors
///
/// An error message, and nothing read, when no level is given.
pub fn read_logs(
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
