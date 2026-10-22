//! The errors of `xtask`.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// A `Result` whose error is an `xtask` [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// Why a command of `xtask` failed.
#[derive(Debug)]
pub enum Error {
    /// The command line was wrong; the text says how.
    Usage(String),
    /// Reading or writing a file, or starting a program, failed.
    Io {
        /// What was being done, for example "reading Cargo.toml".
        context: String,
        /// The error of the operating system.
        source: io::Error,
    },
    /// A program ran and failed.
    Failed {
        /// The command line, as printed before it ran.
        command: String,
    },
    /// A Rust source file could not be parsed.
    Parse {
        /// The file.
        path: PathBuf,
        /// The parser's error.
        source: syn::Error,
    },
    /// Output of `cargo` or a file of `xtask` had an unexpected form.
    Format(String),
    /// A check found problems; each entry is one problem.
    Check {
        /// The check, for example "check-deps".
        check: &'static str,
        /// The problems, one line each.
        problems: Vec<String>,
    },
}

impl Error {
    /// An [`Error::Io`] with `context`.
    pub fn io(context: impl Into<String>, source: io::Error) -> Error {
        Error::Io {
            context: context.into(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(text) | Error::Format(text) => write!(formatter, "{text}"),
            Error::Io { context, .. } => write!(formatter, "{context} failed"),
            Error::Failed { command } => write!(formatter, "`{command}` failed"),
            Error::Parse { path, .. } => {
                write!(formatter, "cannot parse {}", path.display())
            }
            Error::Check { check, problems } => {
                write!(formatter, "{check} found {} problem(s)", problems.len())?;
                for problem in problems {
                    write!(formatter, "\n  {problem}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            Error::Parse { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_lists_every_problem_on_its_own_line() {
        let error = Error::Check {
            check: "check-deps",
            problems: vec!["first".to_owned(), "second".to_owned()],
        };

        assert_eq!(
            error.to_string(),
            "check-deps found 2 problem(s)\n  first\n  second"
        );
    }

    #[test]
    fn an_io_error_keeps_its_cause() {
        let error = Error::io("reading x", io::Error::other("disk on fire"));

        assert_eq!(error.to_string(), "reading x failed");
        let source = std::error::Error::source(&error).map(ToString::to_string);
        assert_eq!(source.as_deref(), Some("disk on fire"));
    }
}
