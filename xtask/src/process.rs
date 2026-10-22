//! Running programs and showing what runs.
//!
//! Every program `xtask` runs is printed first, as `$ program args`, so that
//! a developer can copy the line and run it alone. On GitHub Actions the
//! output of each program is folded into a group named after it.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use crate::error::{Error, Result};

/// A program to run, built like [`std::process::Command`] but printable.
#[derive(Debug)]
pub struct Program {
    /// The command that runs.
    command: Command,
    /// The command line as shown to the developer.
    shown: String,
}

impl Program {
    /// A program named `name`, without arguments, run in `directory`.
    pub fn new(name: &str, directory: &Path) -> Program {
        let mut command = Command::new(name);
        command.current_dir(directory);
        Program {
            command,
            shown: name.to_owned(),
        }
    }

    /// Adds arguments.
    pub fn args<I, S>(mut self, args: I) -> Program
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for arg in args {
            let arg = arg.as_ref();
            self.shown.push(' ');
            self.shown.push_str(&arg.to_string_lossy());
            self.command.arg(arg);
        }
        self
    }

    /// Sets an environment variable, shown before the program name.
    pub fn env(mut self, key: &str, value: &str) -> Program {
        self.shown = format!("{key}={value} {}", self.shown);
        self.command.env(key, value);
        self
    }

    /// The command line as shown to the developer.
    pub fn shown(&self) -> &str {
        &self.shown
    }

    /// Runs the program with its output going to the terminal.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the program cannot start, [`Error::Failed`] if it
    /// exits with a failure.
    pub fn run(mut self) -> Result<()> {
        let group = is_github_actions();
        if group {
            println!("::group::{}", self.shown());
        }
        println!("$ {}", self.shown());
        let status = self.command.status();
        if group {
            println!("::endgroup::");
        }
        let status =
            status.map_err(|error| Error::io(format!("starting `{}`", self.shown), error))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Failed {
                command: self.shown,
            })
        }
    }

    /// Runs the program and returns what it wrote to standard output.
    ///
    /// Standard error goes to the terminal.
    ///
    /// # Errors
    ///
    /// As [`Program::run`].
    pub fn output(mut self) -> Result<String> {
        self.command.stderr(std::process::Stdio::inherit());
        let output = self
            .command
            .output()
            .map_err(|error| Error::io(format!("starting `{}`", self.shown), error))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(Error::Failed {
                command: self.shown,
            })
        }
    }
}

/// Whether `xtask` runs on GitHub Actions.
pub fn is_github_actions() -> bool {
    std::env::var_os("GITHUB_ACTIONS").is_some_and(|value| value == "true")
}

/// Appends Markdown to the summary of the GitHub Actions job, if there is one.
///
/// # Errors
///
/// [`Error::Io`] if the summary file cannot be written.
pub fn job_summary(markdown: &str) -> Result<()> {
    let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") else {
        return Ok(());
    };
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .map_err(|error| Error::io("opening the job summary", error))?;
    std::io::Write::write_all(&mut file, markdown.as_bytes())
        .map_err(|error| Error::io("writing the job summary", error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shown_command_line_has_environment_program_and_arguments() {
        let program = Program::new("cargo", Path::new("."))
            .args(["doc", "--no-deps"])
            .env("RUSTDOCFLAGS", "-D warnings");

        assert_eq!(
            program.shown(),
            "RUSTDOCFLAGS=-D warnings cargo doc --no-deps"
        );
    }

    #[test]
    fn a_failing_program_names_its_command_line() {
        let program = Program::new("false", Path::new("."));

        let error = program.run().unwrap_err();

        assert_eq!(error.to_string(), "`false` failed");
    }

    #[test]
    fn output_returns_standard_output() {
        let program = Program::new("echo", Path::new(".")).args(["hello"]);

        assert_eq!(program.output().unwrap(), "hello\n");
    }
}
