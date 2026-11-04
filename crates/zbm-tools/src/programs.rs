//! Programs named by the configuration or the user: hooks, key scripts, the
//! recovery shell, and ZFSBootMenu's own applets.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::path::Path;

use zbm_platform::shell::{self, Interpreter};
use zbm_platform::{Command, Input, Output, Runner, Signal, Stream};

/// `command` with the console as its standard input, output and error.
fn on_console(command: Command) -> Command {
    command
        .stdin(Input::Inherit)
        .stdout(Stream::Inherit)
        .stderr(Stream::Inherit)
}

/// Runs the hook `hook` on the console with `env`, as Bash runs
/// `"${_hook}"`: its exit status is ignored. It runs as the kernel would run
/// it, told from `start`, its first bytes, or as it is when they could not
/// be read (ADR 0017, point 6): a `#!` line naming `sh` or `bash` runs on
/// the shell the runtime chose ([`zbm_platform::shell`]) with that line's
/// argument, and a text without a `#!` line on that shell, or on Bash.
pub fn run_hook(runner: &dyn Runner, hook: &Path, start: Option<&[u8]>, env: &[(&str, &str)]) {
    let command = hook_command(hook, start, shell::chosen().as_deref());
    let command = env
        .iter()
        .fold(command, |command, (key, value)| command.env(key, value));
    let _ = runner.run(&on_console(command));
}

/// The command that runs `hook`, whose first bytes are `start`, where the
/// runtime chose `shell`: brush keeps a `#!` line's options when it has them,
/// and a line asking for another one runs on Bash.
fn hook_command(hook: &Path, start: Option<&[u8]>, shell: Option<&Path>) -> Command {
    let Some(start) = start else {
        return Command::new(hook);
    };
    match (shell::interpreter(start), shell) {
        (Interpreter::Shell(argument), Some(shell)) => {
            let is_brush = shell.file_name().is_some_and(|name| name == "brush");
            let program = match argument.as_deref() {
                Some(argument) if is_brush && !brush_supports(argument) => {
                    zbm_trace::info!(
                        "{hook} asks for {argument}, which brush lacks, so bash runs it";
                        hook = %hook.display(), argument
                    );
                    Path::new(shell::BASH)
                }
                _ => shell,
            };
            Command::new(program).args(argument).arg(hook)
        }
        (Interpreter::Shell(_) | Interpreter::Kernel, _) => Command::new(hook),
        (Interpreter::Text, shell) => {
            Command::new(shell.unwrap_or(Path::new(shell::BASH))).arg(hook)
        }
    }
}

/// Whether brush, run as `brush ARGUMENT SCRIPT`, takes `argument`, the
/// argument of a script's `#!` line: options of a script that brush has
/// (`-e`, `-f`, `-n`, `-u`, `-v`, `-x`), alone or together (`-eu`).
fn brush_supports(argument: &str) -> bool {
    argument
        .strip_prefix('-')
        .is_some_and(|flags| !flags.is_empty() && flags.chars().all(|flag| "efnuvx".contains(flag)))
}

/// Runs a crypttab key script with its argument `key` and `env`
/// (`CRYPTTAB_NAME` and the like), with the console as its standard input
/// and its output captured as a secret: the key.
///
/// # Errors
///
/// The error that kept the script from starting.
pub fn key_script(
    runner: &dyn Runner,
    script: &str,
    key: &str,
    env: &[(&str, &str)],
) -> io::Result<Output> {
    let command = env
        .iter()
        .fold(Command::new(script).arg(key), |command, (name, value)| {
            command.env(name, value)
        })
        .stdin(Input::Inherit)
        .stdout(Stream::CaptureSecret);
    runner.run(&command)
}

/// Runs an interactive shell on the console, the one the runtime chose
/// ([`zbm_platform::shell`]) or Bash (`/bin/bash -i`), which Ctrl-C reaches
/// with its default action. When it ends, this process takes the terminal
/// back: a shell with job control that ended without giving it back, or
/// was killed, leaves its own process group in the foreground, and the
/// menu or start-up could then read no key.
///
/// # Errors
///
/// The error that kept the shell from starting.
pub fn interactive_shell(runner: &dyn Runner) -> io::Result<Output> {
    let program = shell::chosen().unwrap_or_else(|| shell::BASH.into());
    let command = on_console(Command::new(program).arg("-i")).default_signal(Signal::Interrupt);
    let ran = runner.run(&command);
    // Not a terminal (a script, the tests): nothing to take back.
    let _ = zbm_sys::take_terminal(io::stdin());
    ran
}

/// Runs `zbm lock shell` on the console, as [`interactive_shell`] runs a
/// shell: the prompt of a locked image, which asks for its PIN and opens
/// the shell once the PIN is right (ROADMAP.md SE9).
///
/// # Errors
///
/// The error that kept it from starting.
pub fn locked_shell(runner: &dyn Runner) -> io::Result<Output> {
    let command =
        on_console(Command::new(ZBM).args(["lock", "shell"])).default_signal(Signal::Interrupt);
    let ran = runner.run(&command);
    let _ = zbm_sys::take_terminal(io::stdin());
    ran
}

/// Runs the applet `program` of ZFSBootMenu with `args` and `env` on the
/// console, in a process of its own, such as `zluks unlock`.
///
/// # Errors
///
/// The error that kept it from starting.
pub fn applet(
    runner: &dyn Runner,
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> io::Result<Output> {
    let command = env.iter().fold(
        Command::new(program).args(args.iter().copied()),
        |command, (name, value)| command.env(name, value),
    );
    runner.run(&on_console(command))
}

/// Where the boot image installs `zbm`.
pub const ZBM: &str = "/libexec/zbm";

/// Asks the console on the display to hand the display, the keyboards and
/// the virtual terminal back to the kernel, and waits until it did
/// (`zbm console release`), before `kexec`.
///
/// # Errors
///
/// The error that kept it from starting.
pub fn release_console(runner: &dyn Runner) -> io::Result<Output> {
    runner.run(&on_console(Command::new(ZBM).args(["console", "release"])))
}

/// Asks the console that handed the display back before `kexec` to take
/// it again, once `kexec` failed (`zbm console reattach`).
///
/// # Errors
///
/// The error that kept it from starting.
pub fn reattach_console(runner: &dyn Runner) -> io::Result<Output> {
    runner.run(&on_console(Command::new(ZBM).args(["console", "reattach"])))
}

/// Copies the file `from` to `to` as `cp FROM TO` does, quietly: the copy
/// gets the mode `cp` gives a new file, the source's less the umask.
///
/// # Errors
///
/// The error that kept `cp` from starting.
pub fn copy_file(runner: &dyn Runner, from: &Path, to: &Path) -> io::Result<Output> {
    let command = Command::new("cp")
        .arg(from)
        .arg(to)
        .stdout(Stream::Null)
        .stderr(Stream::Null);
    runner.run(&command)
}

/// Runs the program of `args` with the rest as its arguments and `env`
/// added to its environment, without a shell, with the console as its
/// standard input and what it prints, errors included, captured: the
/// commands of `generate-zbm` (ADR 0013).
///
/// # Errors
///
/// The error that kept the program from starting, such as `NotFound`.
pub fn merged(
    runner: &dyn Runner,
    env: &[(String, String)],
    args: &[String],
) -> io::Result<Output> {
    let Some((program, rest)) = args.split_first() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no program to run",
        ));
    };
    let mut command = Command::new(program)
        .args(rest)
        .stdin(Input::Inherit)
        .merge_stderr();
    for (name, value) in env {
        command = command.env(name, value);
    }
    runner.run(&command)
}

/// Opens `file` in `editor` on the console.
///
/// # Errors
///
/// The error that kept the editor from starting.
pub fn edit(runner: &dyn Runner, editor: &str, file: &Path) -> io::Result<Output> {
    runner.run(&on_console(Command::new(editor).arg(file)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::ffi::OsString;
    use zbm_platform::{Secret, Status};
    use zbm_testkit::FakeRunner;

    /// The command line `hook_command` gives for a hook starting with
    /// `start`, with brush chosen or no shell.
    fn line(start: Option<&[u8]>, shell: Option<&str>) -> String {
        let command = hook_command(Path::new("/hooks/h"), start, shell.map(Path::new));
        std::iter::once(command.program())
            .chain(command.get_args().iter().map(OsString::as_os_str))
            .map(|word| word.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn hooks_run_as_the_kernel_would_run_them() {
        let brush = Some("/usr/bin/brush");
        // Fix 21: the options of a shell's `#!` line are kept.
        assert_eq!(
            line(Some(b"#!/bin/sh -e\n"), brush),
            "/usr/bin/brush -e /hooks/h"
        );
        assert_eq!(
            line(Some(b"#!/bin/bash\n"), brush),
            "/usr/bin/brush /hooks/h"
        );
        // One brush lacks, as the kernel passes it: one argument, to Bash.
        assert_eq!(
            line(Some(b"#!/bin/bash -o pipefail\n"), brush),
            "/bin/bash -o pipefail /hooks/h"
        );
        assert_eq!(
            line(Some(b"#!/bin/bash -o pipefail\n"), Some("/bin/bash")),
            "/bin/bash -o pipefail /hooks/h"
        );
        // Fix 22: an ELF hook, and other interpreters, run as they are.
        assert_eq!(line(Some(b"\x7fELF\x02\x01"), brush), "/hooks/h");
        assert_eq!(line(Some(b"#!/usr/bin/python3\n"), brush), "/hooks/h");
        assert_eq!(line(Some(b"#!/usr/bin/env bash\n"), brush), "/hooks/h");
        // A text without `#!` runs on the shell, never on `/bin/sh`.
        assert_eq!(line(Some(b"echo hi\n"), brush), "/usr/bin/brush /hooks/h");
        assert_eq!(line(Some(b"echo hi\n"), None), "/bin/bash /hooks/h");
        // Without a chosen shell a `#!` line is the kernel's; unread, it
        // runs as it is.
        assert_eq!(line(Some(b"#!/bin/sh -e\n"), None), "/hooks/h");
        assert_eq!(line(None, brush), "/hooks/h");
    }

    #[test]
    fn brush_supports_the_options_of_a_script() {
        for argument in ["-e", "-eu", "-x", "-efnuvx"] {
            assert!(brush_supports(argument), "{argument}");
        }
        for argument in ["", "-", "-o pipefail", "-i", "e", "--posix", "-e -u"] {
            assert!(!brush_supports(argument), "{argument}");
        }
    }

    #[test]
    fn a_hook_runs_on_the_console_with_its_environment() {
        let runner = FakeRunner::new();
        runner.on(|command| {
            (command.program() == "/hooks/missing")
                .then(|| Err(io::Error::from(io::ErrorKind::NotFound)))
        });

        run_hook(
            &runner,
            Path::new("/hooks/plain"),
            Some(b"echo hi\n"),
            &[("ZBM_SELECTED_BE", "tank")],
        );
        run_hook(&runner, Path::new("/hooks/missing"), None, &[]);

        assert_eq!(runner.lines(), ["/bin/bash /hooks/plain", "/hooks/missing"]);
        let calls = runner.calls();
        assert_eq!(
            calls[0].get_env(),
            [("ZBM_SELECTED_BE".into(), Some("tank".into()))]
        );
        assert_eq!(calls[0].get_stdout(), Stream::Inherit);
        // Without a chosen shell, as in these tests.
        assert_eq!(shell::chosen(), None);
    }

    #[test]
    fn key_scripts_shells_and_applets() {
        let runner = FakeRunner::new();
        runner.on(|command| {
            (command.program() == "/etc/keyscript").then(|| {
                Ok(Output::with_secret(
                    Status::exited(0),
                    Secret::copy_from(b"key"),
                ))
            })
        });

        let output = key_script(
            &runner,
            "/etc/keyscript",
            "none",
            &[("CRYPTTAB_NAME", "root")],
        )
        .unwrap();
        interactive_shell(&runner).unwrap();
        applet(&runner, "zluks", &["unlock"], &[("LUKS_RETRY", "1")]).unwrap();

        copy_file(&runner, Path::new("/a"), Path::new("/b")).unwrap();
        edit(&runner, "vi", Path::new("/tmp/kcl")).unwrap();
        merged(
            &runner,
            &[("A".to_owned(), "1".to_owned())],
            &["dracut".to_owned(), "-f".to_owned()],
        )
        .unwrap();
        assert!(merged(&runner, &[], &[]).is_err());
        assert_eq!(output.secret.unwrap().expose(), b"key");
        assert_eq!(
            runner.lines(),
            [
                "/etc/keyscript none",
                "/bin/bash -i",
                "zluks unlock",
                "cp /a /b",
                "vi /tmp/kcl",
                "dracut -f"
            ]
        );
        let calls = runner.calls();
        assert_eq!(calls[0].get_stdout(), Stream::CaptureSecret);
        assert_eq!(calls[1].get_default_signals(), [Signal::Interrupt]);
        assert_eq!(
            calls[2].get_env(),
            [("LUKS_RETRY".into(), Some("1".into()))]
        );
    }
}
