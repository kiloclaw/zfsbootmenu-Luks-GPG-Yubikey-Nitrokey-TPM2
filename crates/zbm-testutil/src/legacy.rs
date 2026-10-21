//! Running the legacy Bash and Perl code, for parity tests.
//!
//! A parity test gives the same input to a legacy function and to its Rust
//! counterpart, and compares the results. The legacy code is the reference:
//! when the two disagree, the Rust code is wrong unless the legacy code has a
//! bug.

use std::path::PathBuf;

use zbm_core::exec::{Command, Input, Output, Runner, Stream, SystemRunner};

/// The path of a file in `legacy/`.
pub fn path(relative: &str) -> PathBuf {
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
        .map(|line| format!("{line}\n"))
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
    pub fn code(mut self, code: &str) -> Bash {
        self.lines.push(code.to_owned());
        self
    }

    /// Sources a library from `legacy/`, for example
    /// `zfsbootmenu/lib/zfsbootmenu-kcl.sh`.
    pub fn source(mut self, library: &str) -> Bash {
        let library = path(library);
        self.lines.push(format!(
            "source {} || exit 99",
            shell_quote(&library.to_string_lossy())
        ));
        self
    }

    /// Sets an environment variable.
    pub fn env(mut self, key: &str, value: &str) -> Bash {
        self.env.push((key.to_owned(), value.to_owned()));
        self
    }

    /// Runs `function` (or any command) with `args`, after everything added so far.
    pub fn call(&self, function: &str, args: &[&str], stdin: &[u8]) -> Output {
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
pub fn perl(program: &str, args: &[&str], stdin: &[u8]) -> Output {
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

/// Quotes `text` for the shell with single quotes.
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_a_legacy_function() {
        let output = Bash::new()
            .code(LOG_STUBS)
            .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh")
            .call("kcl_tokenize", &[], b"a \"b c\" d\n");
        assert!(output.status.success(), "{}", output.stderr_text());
        assert_eq!(output.stdout_text(), "a\n\"b c\"\nd\n");
    }

    #[test]
    fn passes_arguments_unchanged() {
        let output = Bash::new().call("printf", &["[%s]", "a b", "$x", "'q'"], b"");
        assert_eq!(output.stdout_text(), "[a b][$x]['q']");
    }

    #[test]
    fn runs_perl() {
        let output = perl("print join(',', @ARGV)", &["a", "b c"], b"");
        assert_eq!(output.stdout_text(), "a,b c");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }
}
