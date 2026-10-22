//! Running the legacy Bash and Perl code, for parity tests.
//!
//! A parity test gives the same input to a legacy function and to its Rust
//! counterpart, and compares the results. The legacy code is the reference:
//! when the two disagree, the Rust code is wrong unless the legacy code has a
//! bug.
//!
//! These helpers run only inside [`Parity::legacy`](crate::parity::Parity),
//! which records their results as golden files; anywhere else they panic, so
//! that no test depends on the legacy tree at run time.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use zbm_core::exec::{Command, Input, Output, Runner, Status, Stream, SystemRunner};

/// The path of a file in `legacy/`.
///
/// # Panics
///
/// Outside [`Parity::legacy`](crate::parity::Parity::legacy).
pub fn path(relative: &str) -> PathBuf {
    crate::parity::assert_recording();
    crate::repo_root().join("legacy").join(relative)
}

/// Stubs for the logging functions of `kmsg-log-lib.sh`, which write to
/// `/dev/kmsg`: each message goes to standard error as `LEVEL: message`, the
/// format of `echo-log-lib.sh`.
pub const LOG_STUBS: &str = r#"
zdebug() { echo "DEBUG: $*" >&2; }
zinfo() { echo "INFO: $*" >&2; }
znotice() { echo "NOTICE: $*" >&2; }
zwarn() { echo "WARN: $*" >&2; }
zerror() { echo "ERROR: $*" >&2; }
"#;

/// The lines of `stderr` that the [`LOG_STUBS`] wrote, without the shell's own
/// error messages.
pub fn log_lines(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|line| {
            ["DEBUG: ", "INFO: ", "NOTICE: ", "WARN: ", "ERROR: "]
                .iter()
                .any(|label| line.starts_with(label))
        })
        .flat_map(|line| [line, "\n"])
        .collect()
}

/// Log stubs for parity tests whose messages span lines: each line gets the
/// label, as each line of a debugging message gets the prefix in the kernel
/// log. Messages go to the file `ZBM_LOG` names, as the real functions write to
/// `/dev/kmsg` and not to a stream a command substitution could capture, or
/// to standard error without it.
pub const LINE_LOG_STUBS: &str = r#"
_zstub() {
  local _l
  while IFS= read -r _l; do echo "$1: ${_l}"; done <<< "$2" >> "${ZBM_LOG:-/dev/stderr}"
}
zdebug() { _zstub DEBUG "$*"; }
zinfo() { _zstub INFO "$*"; }
znotice() { _zstub NOTICE "$*"; }
zwarn() { _zstub WARN "$*"; }
zerror() { _zstub ERROR "$*"; }
"#;

/// The kernel log as the legacy log stubs print it: `LABEL: message` lines.
/// Continuation lines of messages other than debugging output are left out,
/// as `log_lines` leaves them out.
pub fn stub_lines(kmsg: &str) -> String {
    kmsg.lines()
        .filter_map(|line| {
            if let Some(debug) = line.strip_prefix("<7>ZBM:") {
                let message = debug.splitn(3, '|').nth(2)?;
                return Some(format!("DEBUG: {message}\n"));
            }
            let (level, message) = line.split_once(">ZFSBootMenu: ")?;
            let label = match level {
                "<3" => "ERROR",
                "<4" => "WARN",
                "<5" => "NOTICE",
                _ => "INFO",
            };
            Some(format!("{label}: {message}\n"))
        })
        .collect()
}

/// A Bash program built from legacy libraries, a prelude and a call.
#[derive(Clone, Debug, Default)]
pub struct Bash {
    lines: Vec<String>,
    env: Vec<(String, String)>,
}

impl Bash {
    /// An empty program.
    pub fn new() -> Bash {
        Bash::default()
    }

    /// Adds Bash code, run before anything added later.
    #[must_use]
    pub fn code(mut self, code: &str) -> Bash {
        self.lines.push(code.to_owned());
        self
    }

    /// Sources a library from `legacy/`, for example
    /// `zfsbootmenu/lib/zfsbootmenu-kcl.sh`.
    #[must_use]
    pub fn source(mut self, library: &str) -> Bash {
        let library = path(library);
        self.lines.push(format!(
            "source {} || exit 99",
            shell_quote(&library.to_string_lossy())
        ));
        self
    }

    /// Sets an environment variable.
    #[must_use]
    pub fn env(mut self, key: &str, value: &str) -> Bash {
        self.env.push((key.to_owned(), value.to_owned()));
        self
    }

    /// Runs `function` (or any command) with `args`, after everything added so far.
    ///
    /// # Panics
    ///
    /// Outside [`Parity::legacy`](crate::parity::Parity::legacy), and when
    /// `bash` cannot be started.
    pub fn call(&self, function: &str, args: &[&str], stdin: &[u8]) -> Output {
        crate::parity::assert_recording();
        let mut script = self.lines.join("\n");
        script.push_str("\n\"$@\"\n");
        let mut command = Command::new("bash")
            .arg("-c")
            .arg(script)
            .arg("zbm-parity")
            .arg(function)
            .args(args)
            .stdin(Input::Bytes(stdin.to_vec()))
            .stderr(Stream::Capture)
            .env("LEGACY", path("").to_string_lossy().into_owned())
            .env("LC_ALL", "C");
        for (key, value) in &self.env {
            command = command.env(key, value);
        }
        SystemRunner
            .run(&command)
            .expect("bash can be started for a parity test")
    }
}

/// Runs a Perl program with `args` in `@ARGV`.
///
/// # Panics
///
/// Outside [`Parity::legacy`](crate::parity::Parity::legacy), and when `perl`
/// cannot be started.
pub fn perl(program: &str, args: &[&str], stdin: &[u8]) -> Output {
    crate::parity::assert_recording();
    let command = Command::new("perl")
        .arg("-e")
        .arg(program)
        .arg("--")
        .args(args)
        .stdin(Input::Bytes(stdin.to_vec()))
        .stderr(Stream::Capture)
        .env("LC_ALL", "C");
    SystemRunner
        .run(&command)
        .expect("perl can be started for a parity test")
}

/// What a legacy program did, in the form a golden file records: an
/// [`Output`] with its streams as text.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(from = "RanRecord", into = "RanRecord")]
pub struct Ran {
    /// How the program ended.
    pub status: Status,
    /// Standard output, with invalid UTF-8 replaced.
    pub stdout: String,
    /// Standard error, with invalid UTF-8 replaced.
    pub stderr: String,
}

impl Ran {
    /// Standard output.
    pub fn stdout_text(&self) -> String {
        self.stdout.clone()
    }

    /// Standard error.
    pub fn stderr_text(&self) -> String {
        self.stderr.clone()
    }
}

impl From<Output> for Ran {
    fn from(output: Output) -> Ran {
        Ran {
            status: output.status,
            stdout: output.stdout_text(),
            stderr: output.stderr_text(),
        }
    }
}

/// [`Ran`] as JSON: the raw wait status as a number.
#[derive(Deserialize, Serialize)]
struct RanRecord {
    /// [`Status::raw`].
    status: i32,
    /// Standard output.
    stdout: String,
    /// Standard error.
    stderr: String,
}

impl From<RanRecord> for Ran {
    fn from(record: RanRecord) -> Ran {
        Ran {
            status: Status::from_raw(record.status),
            stdout: record.stdout,
            stderr: record.stderr,
        }
    }
}

impl From<Ran> for RanRecord {
    fn from(ran: Ran) -> RanRecord {
        RanRecord {
            status: ran.status.raw(),
            stdout: ran.stdout,
            stderr: ran.stderr,
        }
    }
}

/// Quotes `text` for the shell with single quotes.
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parity;

    #[test]
    fn runs_a_legacy_function() {
        let mut parity = parity!("runs_a_legacy_function");

        let output: Ran = parity.legacy("kcl_tokenize a \"b c\" d", || {
            Bash::new()
                .code(LOG_STUBS)
                .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh")
                .call("kcl_tokenize", &[], b"a \"b c\" d\n")
                .into()
        });

        assert!(output.status.success(), "{}", output.stderr);
        assert_eq!(output.stdout, "a\n\"b c\"\nd\n");
    }

    #[test]
    fn passes_arguments_unchanged_and_runs_perl() {
        let mut parity = parity!("passes_arguments_unchanged_and_runs_perl");

        let printed: String = parity.legacy("printf [%s] a b, $x, 'q'", || {
            Bash::new()
                .call("printf", &["[%s]", "a b", "$x", "'q'"], b"")
                .stdout_text()
        });
        let joined: String = parity.legacy("perl join a, b c", || {
            perl("print join(',', @ARGV)", &["a", "b c"], b"").stdout_text()
        });

        assert_eq!(printed, "[a b][$x]['q']");
        assert_eq!(joined, "a,b c");
    }

    #[test]
    fn quotes_for_the_shell() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    #[should_panic(expected = "legacy code runs only inside Parity::legacy")]
    fn the_legacy_tree_is_out_of_reach_outside_a_recording() {
        let _ = path("zfsbootmenu");
    }

    #[test]
    fn a_run_round_trips_through_json() {
        let ran = Ran {
            status: Status::exited(3),
            stdout: "out".to_owned(),
            stderr: "err".to_owned(),
        };

        let json = serde_json::to_value(&ran).unwrap();

        assert_eq!(json["status"], 768);
        assert_eq!(serde_json::from_value::<Ran>(json).unwrap(), ran);
    }
}
