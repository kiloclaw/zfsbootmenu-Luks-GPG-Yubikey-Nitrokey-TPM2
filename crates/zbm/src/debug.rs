//! `zbm debug`: what a developer needs from a machine after a failure.
//!
//! - `zbm debug crash` prints the latest crash report in the state directory
//!   (`/zfsbootmenu/crash-TIME-PID.txt`, or `crash-TIME-PID.N.txt` for later
//!   ones of the same process and second), and `zbm debug crash --list` lists
//!   them, oldest first. A developer turns a report's offsets into function
//!   names and lines with `cargo xtask symbolize`.
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

use std::path::Path;

use zbm_platform::{Files, RealFiles};

use crate::{Io, USAGE};

/// The state directory, where crash reports are written.
pub const BASE: &str = "/zfsbootmenu";

/// The usage text.
const USAGE_TEXT: &str = "\
Usage: zbm debug crash [--list]   print the latest crash report, or list them
       zbm debug saved            list the logs saved on the EFI system partition
       zbm debug save             save this boot's logs there now
";

/// The crash reports in `base`, oldest first.
pub fn crash_reports(files: &dyn Files, base: &Path) -> Vec<String> {
    let mut reports: Vec<(u64, String)> = files
        .read_dir(base)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            let name = name.into_string().ok()?;
            let time = name
                .strip_prefix("crash-")?
                .strip_suffix(".txt")?
                .split('-')
                .next()?
                .parse()
                .ok()?;
            Some((time, name))
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
}
