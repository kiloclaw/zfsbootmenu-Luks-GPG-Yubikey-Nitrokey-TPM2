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

use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;

use crate::secret::Secret;

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
    /// without copies.
    Secret(Secret),
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
        }
    }

    /// A command that runs `script` with `sh -c`, as Perl's `qx{}` and `system` do
    /// with a single string.
    pub fn shell(script: impl Into<OsString>) -> Command {
        Command::new("sh").arg("-c").arg(script)
    }

    /// Adds an argument.
    pub fn arg(mut self, arg: impl Into<OsString>) -> Command {
        self.args.push(arg.into());
        self
    }

    /// Adds several arguments.
    pub fn args<I, S>(mut self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Sets standard input.
    pub fn stdin(mut self, stdin: Input) -> Command {
        self.stdin = stdin;
        self
    }

    /// Sets where standard output goes.
    pub fn stdout(mut self, stdout: Stream) -> Command {
        self.stdout = stdout;
        self
    }

    /// Sets where standard error goes.
    pub fn stderr(mut self, stderr: Stream) -> Command {
        self.stderr = stderr;
        self
    }

    /// Sends standard error to the same place as standard output, as `2>&1` does,
    /// keeping the order of the two.
    pub fn merge_stderr(mut self) -> Command {
        self.merge_stderr = true;
        self
    }

    /// Sets an environment variable for the command.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Command {
        self.env.push((key.into(), Some(value.into())));
        self
    }

    /// Removes an environment variable for the command.
    pub fn env_remove(mut self, key: impl Into<OsString>) -> Command {
        self.env.push((key.into(), None));
        self
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
    pub fn success(self) -> bool {
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
    /// Standard output, if it was captured as a secret.
    pub secret: Option<Secret>,
}

impl Output {
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
            secret: Some(secret),
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
        Stream::CaptureSecret => output.secret = Some(Secret::from_vec(message.into_bytes())),
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

impl Runner for SystemRunner {
    fn run(&self, command: &Command) -> io::Result<Output> {
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

        let mut child = match spawn(&mut process) {
            Ok(child) => child,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return not_found(command),
            Err(error) => return Err(error),
        };

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let secret_stdout = command.stdout == Stream::CaptureSecret;

        // Standard input is written, and standard error read, by other threads,
        // so that a command that writes a lot before reading all of its input, or
        // fills one pipe while this thread reads the other, cannot block.
        let (stdout, stderr) = std::thread::scope(|scope| {
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
        });
        // Wait before reporting a read error, so that no child is left behind.
        let status = child.wait()?;

        let mut output = Output::new(Status::from_raw(status.into_raw()), Vec::new());
        match stdout.transpose()? {
            Some(Captured::Plain(bytes)) => output.stdout = bytes,
            Some(Captured::Secret(secret)) => output.secret = Some(secret),
            None => {}
        }
        output.stderr = stderr.transpose()?.unwrap_or_default();
        Ok(output)
    }
}

/// Captured standard output.
enum Captured {
    Plain(Vec<u8>),
    Secret(Secret),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output_and_status() {
        let output = SystemRunner
            .run(&Command::shell("echo out; echo err >&2; exit 3").stderr(Stream::Capture))
            .unwrap();
        assert_eq!(output.stdout, b"out\n");
        assert_eq!(output.stderr, b"err\n");
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(output.status.perl_code(), 3);
        assert_eq!(output.status.shell_code(), 3);
        assert!(!output.status.success());
    }

    #[test]
    fn merged_streams_keep_their_order() {
        let output = SystemRunner
            .run(
                &Command::new("sh")
                    .args(["-c", "echo one; echo two >&2; echo three"])
                    .merge_stderr(),
            )
            .unwrap();
        assert_eq!(output.stdout_text(), "one\ntwo\nthree\n");
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn merged_arguments_are_passed_unchanged() {
        let output = SystemRunner
            .run(
                &Command::new("printf")
                    .args(["[%s]", "a b", "$HOME", "\"q\""])
                    .merge_stderr(),
            )
            .unwrap();
        assert_eq!(output.stdout_text(), "[a b][$HOME][\"q\"]");
    }

    #[test]
    fn feeds_standard_input() {
        let big = "x".repeat(1 << 20);
        let output = SystemRunner
            .run(
                &Command::new("wc")
                    .arg("-c")
                    .stdin(Input::Bytes(big.into_bytes())),
            )
            .unwrap();
        assert_eq!(output.stdout_text().trim(), (1 << 20).to_string());
    }

    #[test]
    fn secrets_travel_through_pipes() {
        let output = SystemRunner
            .run(
                &Command::new("sh")
                    .args(["-c", "tr a-z A-Z; echo done >&2"])
                    .stdin(Input::Secret(Secret::copy_from(b"hunter2")))
                    .stdout(Stream::CaptureSecret)
                    .stderr(Stream::Capture),
            )
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.secret.as_ref().unwrap().expose(), b"HUNTER2");
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"done\n");

        let missing = SystemRunner
            .run(
                &Command::new("zbm-no-such-program")
                    .stdin(Input::Secret(Secret::copy_from(b"x")))
                    .stdout(Stream::CaptureSecret)
                    .merge_stderr(),
            )
            .unwrap();
        assert_eq!(missing.status.code(), Some(127));
        assert!(missing.secret.is_some());
    }

    #[test]
    fn input_the_command_does_not_read_is_not_an_error() {
        // More than a pipe holds, to a program that exits at once.
        let big = vec![b'x'; 1 << 20];
        let output = SystemRunner
            .run(&Command::new("true").stdin(Input::Bytes(big)))
            .unwrap();
        assert!(output.status.success());
        let output = SystemRunner
            .run(&Command::new("false").stdin(Input::Secret(Secret::copy_from(&[0; 1 << 20]))))
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
    }

    #[test]
    fn a_secret_input_does_not_show_in_debug_output() {
        let command = Command::new("cryptsetup")
            .arg("open")
            .stdin(Input::Secret(Secret::copy_from(b"hunter2")));
        assert!(!format!("{command:?}").contains("hunter2"));
        let output = Output::with_secret(Status::exited(0), Secret::copy_from(b"key"));
        assert!(!format!("{output:?}").contains("key\""));
        assert_eq!(output.secret.unwrap().expose(), b"key");
    }

    #[test]
    fn missing_program_reports_127() {
        let output = SystemRunner
            .run(&Command::new("zbm-no-such-program").stderr(Stream::Capture))
            .unwrap();
        assert_eq!(output.status.code(), Some(127));
        assert_eq!(
            output.stderr_text(),
            "zbm-no-such-program: command not found\n"
        );

        let merged = SystemRunner
            .run(&Command::new("zbm-no-such-program").merge_stderr())
            .unwrap();
        assert_eq!(merged.status.code(), Some(127));
    }

    #[test]
    fn signal_statuses_follow_perl_and_shell() {
        let output = SystemRunner.run(&Command::shell("kill -9 $$")).unwrap();
        assert_eq!(output.status.signal(), Some(9));
        assert_eq!(output.status.code(), None);
        assert_eq!(output.status.perl_code(), 0);
        assert_eq!(output.status.shell_code(), 137);
    }

    #[test]
    fn environment_changes_apply() {
        let output = SystemRunner
            .run(
                &Command::shell("echo \"${ZBM_A:-unset} ${HOME:-unset}\"")
                    .env("ZBM_A", "set")
                    .env_remove("HOME"),
            )
            .unwrap();
        assert_eq!(output.stdout_text(), "set unset\n");
    }

    #[test]
    fn display_joins_program_and_arguments() {
        let command = Command::new("zfs").args(["get", "-H", "name"]);
        assert_eq!(command.display(), "zfs get -H name");
        assert_eq!(command.program(), "zfs");
        assert_eq!(command.get_args().len(), 3);
    }
}
