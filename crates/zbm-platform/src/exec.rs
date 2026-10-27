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
use std::io::{self, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use crate::secret::Secret;
pub use zbm_sys::Signal;

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
}

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
pub trait Runner {
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

/// Reads a pipe to its end.
fn read_pipe(mut pipe: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes)?;
    Ok(bytes)
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

/// The process that runs `command`, not yet started.
fn process(command: &Command) -> std::process::Command {
    let mut process = if command.merge_stderr {
        // `exec "$0" "$@" 2>&1` gives the program a single pipe for both streams, so
        // their order is kept. Only messages of the shell itself go to the stderr
        // setting.
        let mut process = std::process::Command::new("sh");
        process
            .arg("-c")
            .arg("exec \"$0\" \"$@\" 2>&1")
            .arg(&command.program)
            .args(&command.args)
            .stderr(stdio(command.stderr));
        process
    } else {
        let mut process = std::process::Command::new(&command.program);
        process.args(&command.args).stderr(stdio(command.stderr));
        process
    };
    process.stdout(stdio(command.stdout));
    if !command.default_signals.is_empty() {
        zbm_sys::restore_in_child(&mut process, &command.default_signals);
    }
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
    process
}

/// What a child wrote to its standard output and standard error.
type Streams = (Option<io::Result<Captured>>, Option<io::Result<Vec<u8>>>);

/// Gives `child` the input of `command` and reads its output.
///
/// Standard input is written, and standard error read, by other threads, so
/// that a command that writes a lot before reading all of its input, or fills
/// one pipe while this thread reads the other, cannot block.
fn communicate(command: &Command, child: &mut std::process::Child) -> Streams {
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let secret_stdout = command.stdout == Stream::CaptureSecret;
    std::thread::scope(|scope| {
        if let Some(mut pipe) = stdin {
            let bytes = match &command.stdin {
                Input::Bytes(bytes) => bytes.as_slice(),
                Input::Secret(secret) => secret.expose(),
                Input::Inherit | Input::Null => &[],
            };
            // A command may exit without reading all of its input; that is not
            // an error here. Closing the pipe ends the input.
            scope.spawn(move || {
                let _ = pipe.write_all(bytes);
            });
        }
        let stderr = stderr.map(|pipe| scope.spawn(move || read_pipe(pipe)));
        let stdout = stdout.map(|mut pipe| {
            if secret_stdout {
                Secret::read_from(&mut pipe).map(Captured::Secret)
            } else {
                read_pipe(pipe).map(Captured::Plain)
            }
        });
        let stderr = stderr.map(|reader| {
            reader
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("reading standard error failed")))
        });
        (stdout, stderr)
    })
}

impl Runner for SystemRunner {
    fn run(&self, command: &Command) -> io::Result<Output> {
        let mut child = match spawn(&mut process(command)) {
            Ok(child) => child,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return not_found(command),
            Err(error) => return Err(error),
        };
        let watchdog = command
            .timeout
            .map(|timeout| Watchdog::start(child.id(), timeout));
        let (stdout, stderr) = communicate(command, &mut child);
        // Wait before reporting a read error, so that no child is left behind.
        let status = match &watchdog {
            Some(watchdog) => watchdog.wait(&mut child)?,
            None => child.wait()?,
        };

        let mut output = Output::new(Status::from_raw(status.into_raw()), Vec::new());
        match stdout.transpose()? {
            Some(Captured::Plain(bytes)) => output.stdout = bytes,
            Some(Captured::Secret(secret)) => output.secret = Some(Arc::new(secret)),
            None => {}
        }
        output.stderr = stderr.transpose()?.unwrap_or_default();
        Ok(output)
    }
}

/// Kills a child that runs past its time. The kill and the collection of
/// the child happen under one lock, so that the signal never reaches
/// another process that got the child's ID once it was collected.
struct Watchdog {
    /// Whether the child was collected.
    collected: Arc<Mutex<bool>>,
    /// Ends the watchdog early, when the child ended in time.
    done: mpsc::Sender<()>,
}

impl Watchdog {
    /// How often a child that closed its output is looked at until it ends.
    const POLL: Duration = Duration::from_millis(5);

    /// Watches the child `pid`, which may run for `timeout`.
    fn start(pid: u32, timeout: Duration) -> Watchdog {
        let collected = Arc::new(Mutex::new(false));
        let (done, ended) = mpsc::channel::<()>();
        let watched = Arc::clone(&collected);
        std::thread::spawn(move || {
            if ended.recv_timeout(timeout) == Err(mpsc::RecvTimeoutError::Timeout) {
                let collected = watched
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if !*collected {
                    let _ = zbm_sys::send_signal(pid, Signal::Kill);
                }
            }
        });
        Watchdog { collected, done }
    }

    /// Waits for `child` to end, collecting it under the lock.
    fn wait(&self, child: &mut std::process::Child) -> io::Result<std::process::ExitStatus> {
        loop {
            {
                let mut collected = self
                    .collected
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(status) = child.try_wait()? {
                    *collected = true;
                    let _ = self.done.send(());
                    return Ok(status);
                }
            }
            std::thread::sleep(Self::POLL);
        }
    }
}

/// Captured standard output.
enum Captured {
    Plain(Vec<u8>),
    Secret(Secret),
}

#[cfg(test)]
mod tests;
