//! `zbm debug`: what a developer needs from a machine after a failure.
//!
//! - `zbm debug crash` prints the latest crash report in the state directory
//!   (`/zfsbootmenu/crash-TIME-PID.txt`, or `crash-TIME-PID.N.txt` for later
//!   ones of the same process and second), and `zbm debug crash --list` lists
//!   them, oldest first. A developer turns a report's offsets into function
//!   names and lines with `cargo xtask symbolize`.
//! - `zbm debug config` prints the options in force and where each came
//!   from, and `zbm debug state` the files of the state directory
//!   ([`files`]).
//! - `zbm debug journal [--level LEVEL]` prints the runtime journal, and
//!   `zbm debug commands` the programs the runtime ran ([`journal`]); they
//!   replace the legacy `zlogtail` and `ztrace`.
//! - `zbm debug luks` and `zbm debug pools` print the LUKS volumes, and the
//!   pools and boot environments, as `zbm-app` reads them ([`machine`]).
//! - `zbm debug saved` lists the logs saved on the EFI system partition
//!   (`zbm.save_logs`), and `zbm debug save` saves this boot's now
//!   ([`crate::saved`]).
//! - `zbm debug panic`, only in builds with the `crash-test` feature, panics,
//!   so that VM tests can check the whole way from a panic to a symbolized
//!   report.
//! - `zbm debug secrets HEX...`, only in builds with the `secret-test`
//!   feature, searches the machine for secrets (`secret_search`).
//! - `zbm debug menu-demo`, only in builds with the `menu-demo` feature, runs
//!   the menu prototype on the display without a terminal
//!   ([`crate::menu_demo`]).

pub mod files;
pub mod journal;
pub mod machine;

use std::path::Path;

use zbm_platform::{Files, RealFiles};

use crate::{Io, USAGE};

/// The state directory, where crash reports are written.
pub const BASE: &str = "/zfsbootmenu";

/// The usage text.
const USAGE_TEXT: &str = "\
Usage: zbm debug crash [--list]   print the latest crash report, or list them
       zbm debug config           the options in force, and where they came from
       zbm debug state            the files of the state directory
       zbm debug journal [--level LEVEL]
                                  the runtime journal (error, warn, info, debug)
       zbm debug commands         the programs the runtime ran, and how they ended
       zbm debug luks             the LUKS volumes and their key methods
       zbm debug pools            the pools and the boot environments
       zbm debug saved            list the logs saved on the EFI system partition
       zbm debug save             save this boot's logs there now
";

/// When the crash report `name` was written, as its name tells:
/// `crash-TIME-PID.txt`, then `crash-TIME-PID.N.txt` (N from 1) for the
/// second and later reports of a process in the same second. Compared as
/// numbers, not as text, where `-100` sorts before `-99` and `.2.txt`
/// before `.txt`.
pub(crate) fn report_order(name: &str) -> Option<(u64, u64, u64)> {
    let stem = name.strip_prefix("crash-")?.strip_suffix(".txt")?;
    let (time, rest) = stem.split_once('-')?;
    let (pid, count) = rest.split_once('.').unwrap_or((rest, "0"));
    Some((time.parse().ok()?, pid.parse().ok()?, count.parse().ok()?))
}

/// The crash reports in `base`, oldest first.
pub fn crash_reports(files: &dyn Files, base: &Path) -> Vec<String> {
    let mut reports: Vec<((u64, u64, u64), String)> = files
        .read_dir(base)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            let name = name.into_string().ok()?;
            Some((report_order(&name)?, name))
        })
        .collect();
    reports.sort();
    reports.into_iter().map(|(_, name)| name).collect()
}

/// `zbm debug` with `files` and the state directory `base`.
pub fn run_in(files: &dyn Files, base: &Path, args: &[String], io: &mut Io) -> i32 {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["crash"] => match crash_reports(files, base).last() {
            Some(name) => match files.read(&base.join(name)) {
                Ok(text) => {
                    let _ = io.stdout.write_all(&text);
                    0
                }
                Err(error) => {
                    let _ = writeln!(io.stderr, "zbm debug: {name}: {error}");
                    1
                }
            },
            None => {
                let _ = writeln!(
                    io.stderr,
                    "zbm debug: no crash report in {}",
                    base.display()
                );
                1
            }
        },
        ["crash", "--list"] => {
            for name in crash_reports(files, base) {
                let _ = writeln!(io.stdout, "{}", base.join(name).display());
            }
            0
        }
        ["config"] => files::config(files, base, io),
        ["state"] => files::state(files, base, io),
        ["commands"] => journal::print_commands(files, base, io),
        ["journal", rest @ ..] => journal::journal(files, base, rest, io).unwrap_or_else(|| {
            let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
            USAGE
        }),
        #[cfg(feature = "crash-test")]
        ["panic"] => panic_for_test(),
        #[cfg(feature = "secret-test")]
        ["secrets", secrets @ ..] => crate::secret_search::run(secrets, io),
        #[cfg(feature = "menu-demo")]
        ["menu-demo", ..] => {
            let rest: Vec<String> = args.iter().skip(1).map(|arg| (*arg).to_owned()).collect();
            crate::menu_demo::run(&rest, io)
        }
        _ => {
            let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
            USAGE
        }
    }
}

/// `zbm debug`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    if let ["luks" | "pools"] = words.as_slice() {
        return machine::run(words[0], io);
    }
    if matches!(words.first(), Some(&("saved" | "save"))) {
        let code = crate::saved::run(Path::new(BASE), &words, io);
        if code == USAGE {
            let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
        }
        return code;
    }
    run_in(&RealFiles, Path::new(BASE), args, io)
}

/// Panics in a function of its own, which a symbolized report must name.
#[cfg(feature = "crash-test")]
#[inline(never)]
#[expect(clippy::panic, reason = "the crash test needs a panic")]
fn panic_for_test() -> i32 {
    // As every applet does, so that the report shows the open span and the
    // kernel log and the journal get the error.
    let _log = zbm_platform::KernelLog::new(&RealFiles, "7", Path::new(BASE), std::process::id())
        .install();
    let _span = tracing::info_span!("crash_test", reason = "zbm debug panic").entered();
    panic!("zbm debug panic: a crash for the crash report test");
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeFiles;

    #[test]
    fn reports_are_ordered_by_time_process_and_number() {
        let files = FakeFiles::new();
        for name in [
            "crash-100-77.1.txt",
            "crash-100-77.txt",
            "crash-100-100.txt",
            "crash-100-99.txt",
            "crash-99-5.txt",
            "crash-100-77.10.txt",
            "crash-100-77.2.txt",
        ] {
            files.file(Path::new(BASE).join(name), "");
        }

        assert_eq!(
            crash_reports(&files, Path::new(BASE)),
            [
                "crash-99-5.txt",
                "crash-100-77.txt",
                "crash-100-77.1.txt",
                "crash-100-77.2.txt",
                "crash-100-77.10.txt",
                "crash-100-99.txt",
                "crash-100-100.txt",
            ]
        );
    }

    fn call(files: &FakeFiles, args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = run_in(
            files,
            Path::new(BASE),
            &args,
            &mut Io {
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        );
        (
            code,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    #[test]
    fn the_latest_report_is_shown_and_all_are_listed_by_time() {
        let files = FakeFiles::new();
        files
            .file("/zfsbootmenu/crash-999999999-7.txt", "old\n")
            .file("/zfsbootmenu/crash-1700000000-12.txt", "new\n")
            .file("/zfsbootmenu/crash-junk.txt", "")
            .file("/zfsbootmenu/journal.jsonl", "");

        assert_eq!(
            call(&files, &["crash"]),
            (0, "new\n".to_owned(), String::new())
        );
        assert_eq!(
            call(&files, &["crash", "--list"]).1,
            "/zfsbootmenu/crash-999999999-7.txt\n/zfsbootmenu/crash-1700000000-12.txt\n"
        );
    }

    #[test]
    fn no_report_and_wrong_usage_are_errors() {
        let files = FakeFiles::new();

        assert_eq!(
            call(&files, &["crash"]),
            (
                1,
                String::new(),
                "zbm debug: no crash report in /zfsbootmenu\n".to_owned()
            )
        );
        assert_eq!(
            call(&files, &["crash", "--list"]),
            (0, String::new(), String::new())
        );
        assert_eq!(call(&files, &[]).0, USAGE);
        assert_eq!(call(&files, &["frobnicate"]).2, USAGE_TEXT);
    }

    #[test]
    fn the_reports_of_the_state_directory_are_reached_by_name() {
        let files = FakeFiles::new();
        files
            .file(
                "/zfsbootmenu/zbm.cmdline",
                "zbm.timeout=5
",
            )
            .file("/zfsbootmenu/journal.jsonl", "");

        assert!(call(&files, &["config"]).1.starts_with(
            "command line: zbm.timeout=5
"
        ));
        assert_eq!(
            call(&files, &["state"]).1,
            "== zbm.cmdline (14 bytes)\nzbm.timeout=5\n"
        );
        assert_eq!(
            call(&files, &["commands"]).1,
            "0 commands, 0 failed or did not start, 0 ms in all\n"
        );
        assert_eq!(call(&files, &["journal"]).0, 1);
        assert_eq!(
            call(&files, &["journal", "--level", "loud"]),
            (USAGE, String::new(), USAGE_TEXT.to_owned())
        );
    }
}
