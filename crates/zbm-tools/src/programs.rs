//! Programs named by the configuration or the user: hooks, key scripts, the
//! recovery shell, and ZFSBootMenu's own applets.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::path::{Path, PathBuf};

use zbm_platform::{Command, Input, Output, Runner, Signal, Stream, shell};

/// The `errno` of a file the kernel cannot run.
const ENOEXEC: i32 = 8;

/// `command` with the console as its standard input, output and error.
fn on_console(command: Command) -> Command {
    command
        .stdin(Input::Inherit)
        .stdout(Stream::Inherit)
        .stderr(Stream::Inherit)
}

/// Runs the hook `hook` on the console with `env`, as Bash runs
/// `"${_hook}"`: its exit status is ignored, and a script the kernel cannot
/// run (no `#!` line) runs with `bash`. Where the runtime chose a shell
/// ([`zbm_platform::shell`]), a hook written for the shell runs on it,
/// whatever its `#!` line names.
pub fn run_hook(runner: &dyn Runner, hook: &Path, env: &[(&str, &str)]) {
    let shell = hook_shell(shell::chosen(), hook);
    run_hook_with(runner, hook, env, shell.as_deref());
}

/// The `shell` chosen, if any, when `hook` is written for it.
fn hook_shell(shell: Option<PathBuf>, hook: &Path) -> Option<PathBuf> {
    shell.filter(|_| first_line(hook).is_some_and(|line| shell::is_shell_script(&line)))
}

/// Runs `hook` with `env`: on `shell` when given, else as the kernel runs it,
/// with `bash` for a script without a `#!` line.
fn run_hook_with(runner: &dyn Runner, hook: &Path, env: &[(&str, &str)], shell: Option<&Path>) {
    let with_env = |command: Command| {
        on_console(
            env.iter()
                .fold(command, |command, (key, value)| command.env(key, value)),
        )
    };
    if let Some(shell) = shell {
        let _ = runner.run(&with_env(Command::new(shell).arg(hook)));
        return;
    }
    if let Err(error) = runner.run(&with_env(Command::new(hook)))
        && error.raw_os_error() == Some(ENOEXEC)
    {
        let _ = runner.run(&with_env(Command::new("bash").arg(hook)));
    }
}

/// The first line of the file at `path`, from its first 256 bytes; `None`
/// when it cannot be read.
fn first_line(path: &Path) -> Option<String> {
    use std::io::Read as _;
    let mut start = [0_u8; 256];
    let length = std::fs::File::open(path).ok()?.read(&mut start).ok()?;
    let text = String::from_utf8_lossy(start.get(..length)?);
    Some(text.lines().next().unwrap_or_default().to_owned())
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

/// Runs the shell command `script` as Perl's `qx{SCRIPT 2>&1}` does
/// (`/bin/sh -c "SCRIPT 2>&1"`), with the console as its standard input and
/// what it prints, errors included, captured; for the commands of
/// `generate-zbm`'s configuration.
///
/// # Errors
///
/// The error that kept the shell from starting.
pub fn shell(runner: &dyn Runner, script: &str) -> io::Result<Output> {
    let command = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("{script} 2>&1"))
        .stdin(Input::Inherit)
        .stdout(Stream::Capture)
        .stderr(Stream::Inherit);
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
    use zbm_platform::{Secret, Status};
    use zbm_testkit::FakeRunner;

    #[test]
    fn hooks_without_an_interpreter_line_run_with_bash() {
        let runner = FakeRunner::new();
        runner.on(|command| {
            (command.program() == "/hooks/plain")
                .then(|| Err(io::Error::from_raw_os_error(ENOEXEC)))
        });
        runner.on(|command| {
            (command.program() == "/hooks/missing")
                .then(|| Err(io::Error::from(io::ErrorKind::NotFound)))
        });

        run_hook(
            &runner,
            Path::new("/hooks/plain"),
            &[("ZBM_SELECTED_BE", "tank")],
        );
        run_hook(&runner, Path::new("/hooks/missing"), &[]);
        run_hook(&runner, Path::new("/hooks/good"), &[]);

        assert_eq!(
            runner.lines(),
            [
                "/hooks/plain",
                "bash /hooks/plain",
                "/hooks/missing",
                "/hooks/good"
            ]
        );
        let calls = runner.calls();
        assert_eq!(
            calls[1].get_env(),
            [("ZBM_SELECTED_BE".into(), Some("tank".into()))]
        );
        assert_eq!(calls[1].get_stdout(), Stream::Inherit);
    }

    #[test]
    fn hooks_for_the_shell_run_on_the_shell_the_runtime_chose() {
        let dir = zbm_testkit::TempDir::new("zbm-tools-hooks");
        let bash = dir.write("bash-hook", "#!/bin/bash\necho hi\n");
        let python = dir.write("python-hook", "#!/usr/bin/python3\nprint('hi')\n");
        let runner = FakeRunner::new();
        let brush = || Some(PathBuf::from("/usr/bin/brush"));

        run_hook_with(&runner, &bash, &[], hook_shell(brush(), &bash).as_deref());
        run_hook_with(
            &runner,
            &python,
            &[],
            hook_shell(brush(), &python).as_deref(),
        );

        assert_eq!(
            runner.lines(),
            [
                format!("/usr/bin/brush {}", bash.display()),
                python.display().to_string()
            ]
        );
        assert_eq!(first_line(&dir.join("missing")), None);
        // Without a chosen shell, as in these tests, hooks run as before.
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
        shell(&runner, "dracut -f").unwrap();
        assert_eq!(output.secret.unwrap().expose(), b"key");
        assert_eq!(
            runner.lines(),
            [
                "/etc/keyscript none",
                "/bin/bash -i",
                "zluks unlock",
                "cp /a /b",
                "vi /tmp/kcl",
                "/bin/sh -c dracut -f 2>&1"
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
