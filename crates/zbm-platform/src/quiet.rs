//! A runner whose programs leave the console alone, for while a front end
//! draws on it, such as the menu's start-up view.

use std::io;

use crate::exec::{Command, Input, Output, Runner, Stream};

/// A [`Runner`] that runs commands through another runner, without the
/// console: what a program would print on it is logged instead, standard
/// error as warnings, and it reads nothing from it. The streams the caller
/// asked for come back as they would.
#[derive(Clone, Copy)]
pub struct QuietRunner<'a> {
    /// The runner that runs the commands.
    inner: &'a dyn Runner,
}

impl<'a> QuietRunner<'a> {
    /// A runner that keeps the programs `inner` runs off the console.
    pub fn new(inner: &'a dyn Runner) -> QuietRunner<'a> {
        QuietRunner { inner }
    }
}

/// Logs each line of `text` that `program` printed, as a warning when it
/// came from standard error.
fn log(program: &str, text: &[u8], warning: bool) {
    for line in String::from_utf8_lossy(text).lines() {
        if line.trim().is_empty() {
            continue;
        }
        if warning {
            zbm_trace::warn!("{program}: {line}");
        } else {
            zbm_trace::info!("{program}: {line}");
        }
    }
}

impl Runner for QuietRunner<'_> {
    fn run(&self, command: &Command) -> io::Result<Output> {
        let stdin = matches!(command.get_stdin(), Input::Inherit);
        let stdout = command.get_stdout() == Stream::Inherit;
        // A command that merges its streams still has the shell's own
        // messages on standard error, such as a program not found.
        let stderr = command.get_stderr() == Stream::Inherit;
        if !(stdin || stdout || stderr) {
            return self.inner.run(command);
        }
        let mut quiet = command.clone();
        if stdin {
            quiet = quiet.stdin(Input::Null);
        }
        if stdout {
            quiet = quiet.stdout(Stream::Capture);
        }
        if stderr {
            quiet = quiet.stderr(Stream::Capture);
        }
        let mut output = self.inner.run(&quiet)?;
        let program = command.program().to_string_lossy();
        if stdout {
            log(&program, &output.stdout, false);
            output.stdout.clear();
        }
        if stderr {
            log(&program, &output.stderr, true);
            output.stderr.clear();
        }
        Ok(output)
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, reason = "the tests run real programs")]
mod tests {
    use super::*;
    use crate::exec::SystemRunner;
    use crate::files::RealFiles;
    use crate::log::KernelLog;
    use zbm_testkit::TempDir;

    #[test]
    fn what_programs_would_print_on_the_console_is_logged() {
        let dir = TempDir::new("zbm-platform-quiet");
        let kmsg = dir.join("kmsg");
        let log = KernelLog::new(&RealFiles, "7", dir.path(), 1).with_device(&kmsg);
        let runner = QuietRunner::new(&SystemRunner);
        let script = "read -r line; echo \"read:$line\"; echo out; echo err >&2";

        let output = {
            let _installed = log.install();
            runner
                .run(
                    &Command::shell(script)
                        .stdin(Input::Inherit)
                        .stdout(Stream::Inherit)
                        .stderr(Stream::Inherit),
                )
                .unwrap()
        };

        assert!(output.status.is_success());
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
        let logged = std::fs::read_to_string(&kmsg).unwrap();
        assert!(logged.contains("<6>ZFSBootMenu: sh: read:\n"), "{logged}");
        assert!(logged.contains("<6>ZFSBootMenu: sh: out\n"), "{logged}");
        assert!(logged.contains("<4>ZFSBootMenu: sh: err\n"), "{logged}");
    }

    /// A runner that answers with what the shell says on standard error,
    /// and records the stream it was asked to put it in.
    struct Shell(std::sync::Mutex<Option<Stream>>);

    impl Runner for Shell {
        fn run(&self, command: &Command) -> io::Result<Output> {
            *self.0.lock().unwrap() = Some(command.get_stderr());
            let mut output = Output::new(crate::exec::Status::exited(2), Vec::new());
            output.stderr = b"sh: cannot open /x: No such file\n".to_vec();
            Ok(output)
        }
    }

    #[test]
    fn the_shell_s_own_messages_of_a_merged_command_are_logged() {
        let dir = TempDir::new("zbm-platform-quiet-merged");
        let kmsg = dir.join("kmsg");
        let log = KernelLog::new(&RealFiles, "7", dir.path(), 1).with_device(&kmsg);
        let shell = Shell(std::sync::Mutex::new(None));
        let output = {
            let _installed = log.install();
            QuietRunner::new(&shell)
                .run(
                    &Command::new("cryptsetup")
                        .merge_stderr()
                        .stdout(Stream::Capture),
                )
                .unwrap()
        };
        assert_eq!(*shell.0.lock().unwrap(), Some(Stream::Capture));
        assert!(output.stderr.is_empty());
        let logged = std::fs::read_to_string(&kmsg).unwrap();
        assert!(
            logged.contains("<4>ZFSBootMenu: cryptsetup: sh: cannot open /x"),
            "{logged}"
        );
    }

    #[test]
    fn streams_the_caller_asked_for_come_back_as_they_would() {
        let runner = QuietRunner::new(&SystemRunner);
        let merged = Command::shell("echo out; echo err >&2")
            .stdin(Input::Null)
            .merge_stderr();
        let output = runner.run(&merged).unwrap();
        assert_eq!(output.stdout_text(), "out\nerr\n");

        let captured = Command::shell("echo out; echo err >&2")
            .stdin(Input::Null)
            .stderr(Stream::Capture);
        let output = runner.run(&captured).unwrap();
        assert_eq!(output.stdout_text(), "out\n");
        assert_eq!(output.stderr_text(), "err\n");
    }
}
