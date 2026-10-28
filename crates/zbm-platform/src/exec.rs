//! Running external programs.
//!
//! Every program ZFSBootMenu runs (`zfs`, `zpool`, `objcopy`, `dracut` and the
//! rest) is described by a [`Command`] and run through a [`Runner`]. Production
//! code uses [`SystemRunner`]; unit tests use a fake runner that records the
//! commands and answers them, so tests can check exactly what would run,
//! including that no secret appears in an argument or the environment.
//!
//! Secrets travel only through pipes: [`Input::Secret`] writes one to a
//! command's standard input, and [`Stream::CaptureSecret`] reads a command's
//! standard output into a [`Secret`].

#![allow(
    clippy::disallowed_methods,
    reason = "SystemRunner is where programs are started"
)]

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use crate::secret::Secret;
pub use zbm_sys::Signal;

use self::streams::{Stdout, communicate};
use self::supervise::Supervisor;
use crate::event::CancelToken;

mod streams;
mod supervise;

/// Where standard output or standard error of a command goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stream {
    /// The stream of the calling process.
    Inherit,
    /// A pipe whose contents end up in [`Output`].
    Capture,
    /// For standard output only: a pipe whose contents end up in
    /// [`Output::secret`], never in a plain buffer. On standard error it acts as
    /// [`Stream::Capture`].
    CaptureSecret,
    /// `/dev/null`.
    Null,
}

/// What a command reads on standard input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Input {
    /// The standard input of the calling process.
    Inherit,
    /// `/dev/null`.
    Null,
    /// The given bytes, followed by end of file.
    Bytes(Vec<u8>),
    /// A secret, followed by end of file. It is written from where it is kept,
    /// without copies; a clone of the command shares it.
    Secret(Arc<Secret>),
}

impl Input {
    /// `secret` as standard input.
    pub fn secret(secret: Secret) -> Input {
        Input::Secret(Arc::new(secret))
    }
}

/// A program to run, with its arguments and standard streams.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    stdin: Input,
    stdout: Stream,
    stderr: Stream,
    merge_stderr: bool,
    env: Vec<(OsString, Option<OsString>)>,
    default_signals: Vec<Signal>,
    timeout: Option<Duration>,
    cancel: Option<CancelToken>,
    output_limit: Option<usize>,
}

/// How many bytes of a command's standard output, and of its standard
/// error, are taken by default: far more than any program the runtime runs
/// writes, and little enough that a program gone wrong cannot fill memory.
pub const OUTPUT_LIMIT: usize = 64 << 20;

/// How many bytes of standard output captured as a secret are taken by
/// default: the largest key file `cryptsetup` reads by default.
pub const SECRET_LIMIT: usize = 8 << 20;

impl Command {
    /// A command that runs `program`, inheriting standard input and standard error
    /// and capturing standard output, as a shell command substitution `$(program)`
    /// does.
    pub fn new(program: impl Into<OsString>) -> Command {
        Command {
            program: program.into(),
            args: Vec::new(),
            stdin: Input::Inherit,
            stdout: Stream::Capture,
            stderr: Stream::Inherit,
            merge_stderr: false,
            env: Vec::new(),
            default_signals: Vec::new(),
            timeout: None,
            cancel: None,
            output_limit: None,
        }
    }

    /// A command that runs `script` with `sh -c`, as Perl's `qx{}` and `system` do
    /// with a single string.
    pub fn shell(script: impl Into<OsString>) -> Command {
        Command::new("sh").arg("-c").arg(script)
    }

    /// Adds an argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Command {
        self.args.push(arg.into());
        self
    }

    /// Adds several arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Sets standard input.
    #[must_use]
    pub fn stdin(mut self, stdin: Input) -> Command {
        self.stdin = stdin;
        self
    }

    /// Sets where standard output goes.
    #[must_use]
    pub fn stdout(mut self, stdout: Stream) -> Command {
        self.stdout = stdout;
        self
    }

    /// Sets where standard error goes.
    #[must_use]
    pub fn stderr(mut self, stderr: Stream) -> Command {
        self.stderr = stderr;
        self
    }

    /// Sends standard error to the same place as standard output, as `2>&1` does,
    /// keeping the order of the two.
    #[must_use]
    pub fn merge_stderr(mut self) -> Command {
        self.merge_stderr = true;
        self
    }

    /// Sets an environment variable for the command.
    #[must_use]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Command {
        self.env.push((key.into(), Some(value.into())));
        self
    }

    /// Removes an environment variable for the command.
    #[must_use]
    pub fn env_remove(mut self, key: impl Into<OsString>) -> Command {
        self.env.push((key.into(), None));
        self
    }

    /// Starts the program with the default action of `signal`, as
    /// `trap - SIG; exec program` does, even when this process ignores it.
    #[must_use]
    pub fn default_signal(mut self, signal: Signal) -> Command {
        self.default_signals.push(signal);
        self
    }

    /// Kills the program (`SIGKILL`) when it runs longer than `timeout`, as
    /// `timeout -s KILL` does; the shorter of two limits holds.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Command {
        self.timeout = Some(self.timeout.map_or(timeout, |old| old.min(timeout)));
        self
    }

    /// How long the program may run, if that is limited.
    pub fn get_timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Kills the program (`SIGKILL`) when `token` is cancelled.
    #[must_use]
    pub fn cancel(mut self, token: &CancelToken) -> Command {
        self.cancel = Some(token.clone());
        self
    }

    /// The token that cancels the program, if any.
    pub fn get_cancel(&self) -> Option<&CancelToken> {
        self.cancel.as_ref()
    }

    /// Takes at most `bytes` of each captured stream instead of
    /// [`OUTPUT_LIMIT`] or [`SECRET_LIMIT`]. A program that writes more gets
    /// its pipe closed, and running it is an error.
    #[must_use]
    pub fn output_limit(mut self, bytes: usize) -> Command {
        self.output_limit = Some(bytes);
        self
    }

    /// How many bytes of `stream`, when it is captured, are taken.
    pub fn get_output_limit(&self, stream: Stream) -> usize {
        let default = if stream == Stream::CaptureSecret {
            SECRET_LIMIT
        } else {
            OUTPUT_LIMIT
        };
        self.output_limit.unwrap_or(default)
    }

    /// The signals the program starts with the default action of.
    pub fn get_default_signals(&self) -> &[Signal] {
        &self.default_signals
    }

    /// The program to run.
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// The arguments.
    pub fn get_args(&self) -> &[OsString] {
        &self.args
    }

    /// The standard input.
    pub fn get_stdin(&self) -> &Input {
        &self.stdin
    }

    /// Where standard output goes.
    pub fn get_stdout(&self) -> Stream {
        self.stdout
    }

    /// Where standard error goes.
    pub fn get_stderr(&self) -> Stream {
        self.stderr
    }

    /// Whether standard error goes to standard output.
    pub fn merges_stderr(&self) -> bool {
        self.merge_stderr
    }

    /// The environment changes for the command.
    pub fn get_env(&self) -> &[(OsString, Option<OsString>)] {
        &self.env
    }

    /// The program and its arguments joined by spaces, for messages and test
    /// assertions.
    pub fn display(&self) -> String {
        std::iter::once(&self.program)
            .chain(&self.args)
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

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

/// Runs commands.
pub trait Runner: Send + Sync {
    /// Runs `command` and waits for it to end.
    ///
    /// A program that cannot be found is reported as a shell would: exit status 127
    /// and a "command not found" message on the command's standard error. Other
    /// failures to start the program are errors.
    ///
    /// # Errors
    ///
    /// When the program cannot be started or its output cannot be read.
    fn run(&self, command: &Command) -> io::Result<Output>;
}

/// Runs commands on the system.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemRunner;

fn stdio(stream: Stream) -> Stdio {
    match stream {
        Stream::Inherit => Stdio::inherit(),
        Stream::Capture | Stream::CaptureSecret => Stdio::piped(),
        Stream::Null => Stdio::null(),
    }
}

fn not_found(command: &Command) -> io::Result<Output> {
    let message = format!("{}: command not found\n", command.program.to_string_lossy());
    let mut output = Output::new(Status::exited(127), Vec::new());
    let target = if command.merge_stderr {
        command.stdout
    } else {
        command.stderr
    };
    match target {
        Stream::Inherit => io::stderr().write_all(message.as_bytes())?,
        Stream::CaptureSecret => {
            output.secret = Some(Arc::new(Secret::from_vec(message.into_bytes())));
        }
        Stream::Capture if command.merge_stderr => output.stdout = message.into_bytes(),
        Stream::Capture => output.stderr = message.into_bytes(),
        Stream::Null => {}
    }
    Ok(output)
}

/// `ETXTBSY`: the program is open for writing somewhere.
const TEXT_FILE_BUSY: i32 = 26;

/// Starts a process, retrying for a moment when the program is busy: a program
/// written just before it runs can still be open in a child that another thread
/// is starting.
fn spawn(process: &mut std::process::Command) -> io::Result<std::process::Child> {
    let mut attempts = 0;
    loop {
        match process.spawn() {
            Err(error) if error.raw_os_error() == Some(TEXT_FILE_BUSY) && attempts < 50 => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

/// The process that runs `command`, not yet started, and the read end of
/// the pipe that takes both of its output streams when standard error is
/// merged into captured standard output.
fn process(command: &Command) -> io::Result<(std::process::Command, Option<io::PipeReader>)> {
    let mut process = std::process::Command::new(&command.program);
    process.args(&command.args);
    let mut merged = None;
    if command.merge_stderr {
        // `2>&1`: one pipe, or one file, for both streams, so that their
        // order is kept.
        match command.stdout {
            Stream::Capture | Stream::CaptureSecret => {
                let (reader, writer) = io::pipe()?;
                process
                    .stdout(Stdio::from(writer.try_clone()?))
                    .stderr(Stdio::from(writer));
                merged = Some(reader);
            }
            Stream::Inherit => {
                let stdout = std::os::fd::AsFd::as_fd(&io::stdout()).try_clone_to_owned()?;
                process.stdout(Stdio::inherit()).stderr(Stdio::from(stdout));
            }
            Stream::Null => {
                process.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
    } else {
        process
            .stdout(stdio(command.stdout))
            .stderr(stdio(command.stderr));
    }
    let setup = zbm_sys::ChildSetup {
        default_signals: command.default_signals.clone(),
        ..zbm_sys::ChildSetup::default()
    };
    zbm_sys::prepare_child(&mut process, &setup);
    process.stdin(match command.stdin {
        Input::Inherit => Stdio::inherit(),
        Input::Null => Stdio::null(),
        Input::Bytes(_) | Input::Secret(_) => Stdio::piped(),
    });
    for (key, value) in &command.env {
        match value {
            Some(value) => process.env(key, value),
            None => process.env_remove(key),
        };
    }
    Ok((process, merged))
}

impl Runner for SystemRunner {
    fn run(&self, command: &Command) -> io::Result<Output> {
        let (mut process, merged) = process(command)?;
        let spawned = spawn(&mut process);
        // The child has its copies of the merged pipe's write end: this
        // process's must close, or reading the pipe would never end.
        drop(process);
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return not_found(command),
            Err(error) => return Err(error),
        };
        let supervisor = Supervisor::new(command, &child);
        let (stdout, stderr) = communicate(command, &mut child, merged, supervisor.as_ref());
        // Wait before reporting a read error, so that no child is left behind.
        let status = match &supervisor {
            Some(supervisor) => supervisor.wait(&mut child)?,
            None => child.wait()?,
        };

        let mut output = Output::new(Status::from_raw(status.into_raw()), Vec::new());
        let mut over = false;
        match stdout.transpose()? {
            Some(Stdout::Plain(bytes)) => {
                over |= bytes.over;
                output.stdout = bytes.data;
            }
            Some(Stdout::Secret(secret)) => {
                over |= secret.over;
                output.secret = Some(Arc::new(secret.data));
            }
            None => {}
        }
        if let Some(bytes) = stderr.transpose()? {
            over |= bytes.over;
            output.stderr = bytes.data;
        }
        if over {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{}: more output than the {} bytes taken",
                    command.program.to_string_lossy(),
                    command.get_output_limit(command.stdout)
                ),
            ));
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests;
