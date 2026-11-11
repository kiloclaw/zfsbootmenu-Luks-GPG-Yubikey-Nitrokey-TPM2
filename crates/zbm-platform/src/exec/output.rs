//! What a program that ran produced: [`Output`], and how it ended,
//! [`Status`].

use std::os::unix::process::ExitStatusExt;
use std::sync::Arc;

use crate::secret::Secret;

/// How a command ended, as the raw status `waitpid` reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status(i32);

impl Status {
    /// A status from the raw value `waitpid` reports.
    pub fn from_raw(raw: i32) -> Status {
        Status(raw)
    }

    /// The status of a command that exited with `code`.
    pub fn exited(code: u8) -> Status {
        Status(i32::from(code) << 8)
    }

    /// The raw value `waitpid` reports.
    pub fn raw(self) -> i32 {
        self.0
    }

    /// Whether the command exited with status 0.
    pub fn is_success(self) -> bool {
        self.0 == 0
    }

    /// The exit code, if the command exited.
    pub fn code(self) -> Option<i32> {
        std::process::ExitStatus::from_raw(self.0).code()
    }

    /// The signal that ended the command, if one did.
    pub fn signal(self) -> Option<i32> {
        std::process::ExitStatus::from_raw(self.0).signal()
    }

    /// Perl's `$? >> 8`: the exit code, or 0 when a signal ended the command.
    pub fn perl_code(self) -> i32 {
        self.0 >> 8
    }

    /// The status a shell reports in `$?`: the exit code, or 128 plus the signal
    /// number.
    pub fn shell_code(self) -> i32 {
        self.code()
            .or_else(|| self.signal().map(|signal| 128 + signal))
            .unwrap_or(1)
    }
}

/// What a command produced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Output {
    /// How the command ended.
    pub status: Status,
    /// Standard output, if it was captured (with standard error when merged).
    pub stdout: Vec<u8>,
    /// Standard error, if it was captured.
    pub stderr: Vec<u8>,
    /// Standard output, if it was captured as a secret; a clone of the
    /// output shares it.
    pub secret: Option<Arc<Secret>>,
}

impl Output {
    /// The secret it captured, taken out of the output. Where a clone of the
    /// output still shares it, which only tests do, it is copied.
    pub fn into_secret(self) -> Option<Secret> {
        self.secret.map(crate::secret::unshare)
    }

    /// An output with the given status and captured standard output.
    pub fn new(status: Status, stdout: impl Into<Vec<u8>>) -> Output {
        Output {
            status,
            stdout: stdout.into(),
            stderr: Vec::new(),
            secret: None,
        }
    }

    /// An output with the given status and standard output captured as a
    /// secret.
    pub fn with_secret(status: Status, secret: Secret) -> Output {
        Output {
            secret: Some(Arc::new(secret)),
            ..Output::new(status, Vec::new())
        }
    }

    /// Standard output as text, with invalid UTF-8 replaced.
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Standard error as text, with invalid UTF-8 replaced.
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}
