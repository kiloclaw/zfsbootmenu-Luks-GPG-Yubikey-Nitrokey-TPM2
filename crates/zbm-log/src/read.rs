//! Reading the kernel log back, as `print_kmsg_logs` does for `zlogtail`.
//!
//! util-linux `dmesg` filters by level and time itself; its timestamps are
//! removed from the output. When that fails (BusyBox `dmesg` has no such
//! options), the raw log is filtered by the level prefixes of its lines
//! instead.

use zbm_core::exec::{Command, Runner, Stream};
use zbm_core::shell::command_substitution;

use crate::Message;

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
                return &line[end + 2..];
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
        let printed: String = fields
            .iter()
            .skip(2)
            .map(|field| format!("{field} "))
            .collect();
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
/// Returns an error message, and reads nothing, when no level is given.
pub fn read_logs(
    runner: &dyn Runner,
    levels: &str,
    since: Option<&str>,
) -> Result<String, Message> {
    if levels.is_empty() {
        return Err(Message::error("levels is undefined"));
    }
    if let Ok(output) = runner.run(&filtered_command(levels, since))
        && output.status.success()
    {
        // `echo "${output}" | sed ...`
        let text = format!("{}\n", command_substitution(&output.stdout_text()));
        return Ok(strip_timestamps(&text));
    }
    let raw = runner
        .run(&Command::new("dmesg").arg("-r"))
        .map(|output| output.stdout_text())
        .unwrap_or_default();
    Ok(filter_raw(&raw, levels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::legacy::{Bash, LOG_STUBS};
    use zbm_testkit::{FakeRunner, TempDir};

    #[test]
    fn filtered_command_splits_the_time() {
        assert_eq!(
            filtered_command("err,warn", None).display(),
            "dmesg -f user --color=never -l err,warn"
        );
        assert_eq!(
            filtered_command("info", Some("2024-01-02 10:00")).display(),
            "dmesg -f user --color=never -l info --since 2024-01-02 10:00"
        );
        assert_eq!(filtered_command("info", Some("")).get_args().len(), 5);
    }

    #[test]
    fn timestamps_are_removed_up_to_the_last_bracket() {
        assert_eq!(
            strip_timestamps("[    1.5] ZFSBootMenu: a\nplain\n[x] [y] b\n[no end\n"),
            "ZFSBootMenu: a\nplain\nb\n[no end\n"
        );
    }

    #[test]
    fn raw_log_is_filtered_by_level_prefix() {
        let raw = "<3>[    1.0] ZFSBootMenu: broken\n<11>[ 2.0] ZFSBootMenu: busybox\n\
                   <6>[ 3.0] ZFSBootMenu: info\n<7>[ 4.0] ZBM:[1]||debug\n\
                   <3>[ 5.0] kernel: other\n";
        assert_eq!(
            filter_raw(raw, "err"),
            "ZFSBootMenu: broken \nZFSBootMenu: busybox \n"
        );
        assert_eq!(filter_raw(raw, "debug"), "ZBM:[1]||debug \n");
        assert_eq!(filter_raw(raw, "bogus").lines().count(), 4);
        assert_eq!(
            filter_raw("<3>[ 1] ZFSBootMenu: and ZBM: both\n", "err"),
            "ZFSBootMenu: and ZBM: both \nZFSBootMenu: and ZBM: both \n"
        );
        assert_eq!(raw_prefixes("err,"), ["<11>", "<3>"]);
        assert_eq!(
            raw_prefixes("err,,warn"),
            ["<11>", "<3>", "", "<12>", "<4>"]
        );
    }

    #[test]
    fn read_logs_prefers_filtered_dmesg() {
        let runner = FakeRunner::new();
        runner.respond(
            "dmesg -f user --color=never -l err",
            0,
            "[  1.0] ZFSBootMenu: one\n\n",
        );
        assert_eq!(
            read_logs(&runner, "err", None).unwrap(),
            "ZFSBootMenu: one\n"
        );
        assert_eq!(runner.lines(), ["dmesg -f user --color=never -l err"]);
        assert_eq!(
            read_logs(&runner, "", None),
            Err(Message::error("levels is undefined"))
        );
    }

    #[test]
    fn read_logs_falls_back_to_the_raw_log() {
        let runner = FakeRunner::new();
        runner.respond("dmesg -f user --color=never -l warn", 1, "");
        runner.respond("dmesg -r", 0, "<4>[ 1] ZFSBootMenu: careful\n<6>[ 2] x\n");
        assert_eq!(
            read_logs(&runner, "warn", None).unwrap(),
            "ZFSBootMenu: careful \n"
        );
    }

    /// Runs `print_kmsg_logs` with a fake `dmesg` that prints `raw` for `dmesg -r`
    /// and `filtered` (or fails when it is `None`) for the filtering call.
    fn legacy_read(levels: &str, since: &str, filtered: Option<&str>, raw: &str) -> String {
        let dir = TempDir::new("zbm-log-parity");
        dir.write("filtered", filtered.unwrap_or_default());
        dir.write("raw", raw);
        let script = format!(
            "#!/bin/sh\necho \"$*\" >> {calls}\n\
             if [ \"$1\" = -r ]; then cat {raw}; exit 0; fi\n{filtered}\n",
            calls = dir.join("calls").display(),
            raw = dir.join("raw").display(),
            filtered = if filtered.is_some() {
                format!("cat {}", dir.join("filtered").display())
            } else {
                "exit 1".to_owned()
            },
        );
        dir.write_executable("bin/dmesg", script);
        let path = format!(
            "{}:{}",
            dir.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = Bash::new()
            .env("PATH", &path)
            .code(LOG_STUBS)
            .code("FORCE_RELOAD=1")
            .source("zfsbootmenu/lib/kmsg-log-lib.sh")
            .code(LOG_STUBS)
            .call("print_kmsg_logs", &[levels, since], b"");
        assert!(output.status.success(), "{}", output.stderr_text());
        output.stdout_text()
    }

    #[test]
    fn read_logs_matches_print_kmsg_logs() {
        let raw = "<3>[    1.0] ZFSBootMenu: broken\n<11>[ 2.0] ZFSBootMenu: busybox\n\
                   <6>[ 3.0] ZFSBootMenu: info here\n<7>[ 4.0] ZBM:[1]|t|debug\n\
                   <3>[ 5.0] kernel: other\n<5>ZFSBootMenu: short\n\n";
        let filtered = "[    1.0] ZFSBootMenu: broken\n[ 3.0] ZFSBootMenu: info\n";
        let cases = [
            ("err", "", Some(filtered)),
            ("err,warn", "2024-01-02 10:00", Some(filtered)),
            ("info", "", Some("")),
            ("err", "", None),
            ("err,warn,notice,info,debug", "", None),
            ("debug,", "", None),
            ("unknown", "", None),
        ];
        for (levels, since, filtered) in cases {
            let runner = FakeRunner::new();
            let since_arg = (!since.is_empty()).then_some(since);
            let filtered_line = filtered_command(levels, since_arg).display();
            match filtered {
                Some(text) => runner.respond(&filtered_line, 0, text),
                None => runner.respond(&filtered_line, 1, ""),
            }
            runner.respond("dmesg -r", 0, raw);
            let rust = read_logs(&runner, levels, since_arg).unwrap();
            let legacy = legacy_read(levels, since, filtered, raw);
            assert_eq!(rust, legacy, "levels {levels:?}, since {since:?}");
        }
    }
}
