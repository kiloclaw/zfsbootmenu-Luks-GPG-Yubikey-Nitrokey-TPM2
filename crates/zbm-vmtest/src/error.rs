//! What can stop a test before its end.

use std::io;
use std::path::PathBuf;

/// Why a test could not go on. The runner records it as a failed check, so
/// that the test still has a result.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file or a socket failed.
    #[error("{what}: {source}")]
    Io {
        /// What was being done.
        what: String,
        /// The cause.
        source: io::Error,
    },
    /// None of the patterns appeared on the console in time.
    #[error("timeout after {seconds} s waiting for {patterns:?}; the console last showed {tail:?}")]
    Timeout {
        /// The patterns.
        patterns: Vec<String>,
        /// How long the test waited.
        seconds: u64,
        /// The end of what the console showed.
        tail: String,
    },
    /// The virtual machine stopped while the test waited for a pattern.
    #[error("the virtual machine stopped while waiting for {patterns:?}; it last showed {tail:?}")]
    Eof {
        /// The patterns.
        patterns: Vec<String>,
        /// The end of what the console showed.
        tail: String,
    },
    /// A pattern is not a valid regular expression.
    #[error("invalid pattern {pattern:?}: {source}")]
    Pattern {
        /// The pattern.
        pattern: String,
        /// The cause.
        source: regex::Error,
    },
    /// A program on the host failed.
    #[error("{command} failed ({status}): {output}")]
    Command {
        /// The command line.
        command: String,
        /// How it ended.
        status: String,
        /// What it printed.
        output: String,
    },
    /// An image or the boot environment failed to build.
    #[error("{what} failed to build; see {log}")]
    Build {
        /// What was built.
        what: String,
        /// Its build log.
        log: PathBuf,
    },
    /// A setup command in the virtual machine failed, or the test found
    /// something it cannot go on without.
    #[error("{0}")]
    Setup(String),
}

impl Error {
    /// An [`Error::Io`] for `what`.
    pub fn io(what: impl Into<String>) -> impl FnOnce(io::Error) -> Error {
        let what = what.into();
        move |source| Error::Io { what, source }
    }
}

/// The result of a step of a test.
pub type Result<T> = std::result::Result<T, Error>;
