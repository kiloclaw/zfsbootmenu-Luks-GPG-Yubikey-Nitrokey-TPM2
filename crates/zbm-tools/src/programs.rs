//! Programs named by the configuration or the user: hooks, key scripts, the
//! recovery shell, and ZFSBootMenu's own applets.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::path::Path;

use zbm_platform::{BASH_MATCH, Command, Input, Output, Runner, Signal, Stream};

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
/// run (no `#!` line) runs with `bash`.
pub fn run_hook(runner: &dyn Runner, hook: &Path, env: &[(&str, &str)]) {
    let with_env = |command: Command| {
        on_console(
            env.iter()
                .fold(command, |command, (key, value)| command.env(key, value)),
        )
    };
    if let Err(error) = runner.run(&with_env(Command::new(hook)))
        && error.raw_os_error() == Some(ENOEXEC)
    {
        let _ = runner.run(&with_env(Command::new("bash").arg(hook)));
    }
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

/// Runs an interactive Bash on the console (`/bin/bash -i`), which Ctrl-C
/// reaches with its default action.
///
/// # Errors
///
/// The error that kept Bash from starting.
pub fn interactive_shell(runner: &dyn Runner) -> io::Result<Output> {
    let command = on_console(Command::new("/bin/bash").arg("-i")).default_signal(Signal::Interrupt);
    runner.run(&command)
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

/// Whether `text` matches the extended regular expression `pattern`, as
/// Bash's `[[ text =~ pattern ]]` decides it. Bash runs the match
/// ([`BASH_MATCH`]), so the regular expressions mean exactly what they mean
/// to the legacy code; `None` if Bash cannot be run.
pub fn bash_regex_match(runner: &dyn Runner, text: &str, pattern: &str) -> Option<bool> {
    let command = Command::new("bash")
        .args(["-c", BASH_MATCH, "bash", text, pattern])
        .stdin(Input::Null)
        .stdout(Stream::Null)
        .stderr(Stream::Null)
        .env("LC_ALL", "C");
    let output = runner.run(&command).ok()?;
    match output.status.code() {
        Some(0) => Some(true),
        // 2: the pattern is not a valid regular expression, which matches nothing
        Some(1 | 2) => Some(false),
        _ => None,
    }
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

    #[test]
    fn bash_decides_regular_expression_matches() {
        let runner = FakeRunner::new();
        runner.on(|command| {
            let matched = command
                .get_args()
                .last()
                .is_some_and(|pattern| pattern == "6");
            Some(Ok(Output::new(Status::exited(u8::from(!matched)), "")))
        });

        assert_eq!(bash_regex_match(&runner, "vmlinuz-6.1", "6"), Some(true));
        assert_eq!(bash_regex_match(&runner, "vmlinuz-6.1", "7"), Some(false));
        let broken = FakeRunner::new();
        broken.respond_program("bash", 2, "");
        assert_eq!(bash_regex_match(&broken, "x", "("), Some(false));
        let killed = FakeRunner::new();
        killed.respond_program("bash", 130, "");
        assert_eq!(bash_regex_match(&killed, "x", "x"), None);
        let calls = runner.calls();
        assert_eq!(calls[0].get_env(), [("LC_ALL".into(), Some("C".into()))]);
    }
}
