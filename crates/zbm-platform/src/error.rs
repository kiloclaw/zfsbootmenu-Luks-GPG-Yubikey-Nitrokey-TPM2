//! The error type of the runtime.
//!
//! Library code returns [`Result`]. An error's text is the message the legacy
//! code logs as an error at the same point; [`Error::detail`] is what it logs
//! for debugging, such as the output of a failed command. The caller decides
//! what happens next: log and continue, tell the user, or open the recovery
//! shell (see `crates/DESIGN.md`).

use std::fmt;
use std::io;

use crate::exec::{Command, Output, Status};
use zbm_core::fs::strerror;

/// A failure of the runtime.
#[derive(Debug)]
pub enum Error {
    /// A file or device operation failed.
    Io {
        /// What was attempted, such as "unable to write /zfsbootmenu/bootfs".
        context: String,
        /// The error of the operation.
        source: io::Error,
    },
    /// A program ended unsuccessfully.
    Failed {
        /// The program and its arguments; never its input, which may be secret.
        command: String,
        /// How it ended.
        status: Status,
        /// What it wrote to its captured output streams.
        output: String,
    },
    /// Input that cannot be used, with the message to log.
    Invalid(String),
    /// The user cancelled.
    Cancelled,
}

/// The result of runtime operations.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// An error for a failed file or device operation.
    pub fn io(context: impl Into<String>, source: io::Error) -> Error {
        Error::Io {
            context: context.into(),
            source,
        }
    }

    /// An error for a command that ended unsuccessfully.
    pub fn failed(command: &Command, output: &Output) -> Error {
        let mut text = output.stdout_text();
        text.push_str(&output.stderr_text());
        Error::Failed {
            command: command.display(),
            status: output.status,
            output: text,
        }
    }

    /// An error for unusable input.
    pub fn invalid(message: impl Into<String>) -> Error {
        Error::Invalid(message.into())
    }

    /// Details worth a debugging message: a failed command's output.
    pub fn detail(&self) -> Option<&str> {
        match self {
            Error::Failed { output, .. } if !output.trim().is_empty() => Some(output.trim_end()),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { context, source } => write!(formatter, "{context}: {}", strerror(source)),
            Error::Failed {
                command, status, ..
            } => match (status.code(), status.signal()) {
                (_, Some(signal)) => write!(formatter, "{command} was killed by signal {signal}"),
                (Some(code), None) => write!(formatter, "{command} failed with status {code}"),
                (None, None) => write!(formatter, "{command} failed"),
            },
            Error::Invalid(message) => formatter.write_str(message),
            Error::Cancelled => formatter.write_str("cancelled"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Checks that a command succeeded, turning its output into an error if not.
///
/// # Errors
///
/// [`Error::Failed`] when the command did not succeed.
pub fn check(command: &Command, output: Output) -> Result<Output> {
    if output.status.is_success() {
        Ok(output)
    } else {
        Err(Error::failed(command, &output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::Input;
    use crate::secret::Secret;

    #[test]
    fn messages() {
        let error = Error::io(
            "unable to write /zfsbootmenu/bootfs",
            io::Error::from_raw_os_error(28),
        );
        assert_eq!(
            error.to_string(),
            "unable to write /zfsbootmenu/bootfs: No space left on device"
        );
        assert!(std::error::Error::source(&error).is_some());
        assert_eq!(error.detail(), None);

        assert_eq!(
            Error::invalid("unknown option 'foo'").to_string(),
            "unknown option 'foo'"
        );
        assert_eq!(Error::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn failed_commands_name_the_command_but_not_its_input() {
        let command = Command::new("cryptsetup")
            .args(["open", "--key-file=-", "/dev/vda2", "luks-1"])
            .stdin(Input::Secret(Secret::copy_from(b"hunter2")));
        let mut output = Output::new(Status::exited(2), "");
        output.stderr = b"No key available with this passphrase.\n".to_vec();
        let error = Error::failed(&command, &output);
        assert_eq!(
            error.to_string(),
            "cryptsetup open --key-file=- /dev/vda2 luks-1 failed with status 2"
        );
        assert_eq!(
            error.detail(),
            Some("No key available with this passphrase.")
        );
        assert!(!format!("{error:?}").contains("hunter2"));

        let killed = Error::failed(&command, &Output::new(Status::from_raw(9), ""));
        assert_eq!(
            killed.to_string(),
            "cryptsetup open --key-file=- /dev/vda2 luks-1 was killed by signal 9"
        );
        assert_eq!(killed.detail(), None);
    }

    #[test]
    fn check_passes_successes_through() {
        let command = Command::new("zpool").arg("list");
        let output = check(&command, Output::new(Status::exited(0), "tank\n")).unwrap();
        assert_eq!(output.stdout_text(), "tank\n");
        let error = check(&command, Output::new(Status::exited(1), "")).unwrap_err();
        assert_eq!(error.to_string(), "zpool list failed with status 1");
    }
}
