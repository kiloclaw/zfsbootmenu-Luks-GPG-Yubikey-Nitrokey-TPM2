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

mod chroot;

pub use self::chroot::Chroot;

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
/// and a line asking for another one runs as the kernel runs it, on the
/// interpreter it names, which then takes the argument or refuses it.
fn hook_command(hook: &Path, start: Option<&[u8]>, shell: Option<&Path>) -> Command {
    let Some(start) = start else {
        return Command::new(hook);
    };
    match (shell::interpreter(start), shell) {
        (Interpreter::Shell(argument), Some(shell)) => {
            let is_brush = shell.file_name().is_some_and(|name| name == "brush");
            match argument.as_deref() {
                Some(argument) if is_brush && !brush_supports(argument) => {
                    zbm_trace::info!(
                        "{hook}: brush lacks {argument}, so its #! line runs it";
                        hook = %hook.display(), argument
                    );
                    Command::new(hook)
                }
                _ => Command::new(shell).args(argument).arg(hook),
            }
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

/// Where an interactive shell runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellRoot<'a> {
    /// In the image's own root: the shell the runtime chose.
    Image,
    /// In a chroot of a filesystem, mounted and ready, with the shell that
    /// runs there.
    Chroot(&'a Chroot),
}

/// Runs an interactive shell on the console, in `root`: in the image, the
/// one the runtime chose ([`zbm_platform::shell`]) or Bash
/// (`/bin/bash -i`); in a chroot, the chroot's shell, in its root, with a
/// prompt that names the filesystem and no `PROMPT_COMMAND`.
/// Ctrl-C reaches it with its default action. When it ends, this process
/// takes the terminal back: a shell with job control that ended without
/// giving it back, or was killed, leaves its own process group in the
/// foreground, and the menu or start-up could then read no key.
///
/// # Errors
///
/// The error that kept the shell from starting.
pub fn interactive_shell(runner: &dyn Runner, root: ShellRoot<'_>) -> io::Result<Output> {
    let command = match root {
        ShellRoot::Image => {
            let program = shell::chosen().unwrap_or_else(|| shell::BASH.into());
            Command::new(program).arg("-i")
        }
        ShellRoot::Chroot(chroot) => chroot.command(),
    };
    let command = on_console(command).default_signal(Signal::Interrupt);
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

/// Runs `program` with `args` and no streams, its failures ignored, as
/// the legacy hooks ran theirs with `>/dev/null 2>&1 || true`.
pub fn run_quietly(runner: &dyn Runner, program: &str, args: &[&str]) {
    let command = Command::new(program)
        .args(args.iter().copied())
        .stdin(Input::Null)
        .stdout(Stream::Null)
        .stderr(Stream::Null);
    let _ = runner.run(&command);
}

/// Where Rust images keep the legacy files of the hook contract's library,
/// for the functions `zbm zfunc` runs as the legacy runtime did (ADR 0017).
pub const LEGACY_LIBRARY: &str = "/usr/share/zfsbootmenu/legacy";

/// The Bash program that runs a function of the legacy library, as the
/// legacy `/libexec/zfunc` did, from the legacy files that Rust images keep
/// below `$1` (`/usr/share/zfsbootmenu/legacy`): the function, then its
/// arguments, follow. The legacy files source each other there, not the
/// generated library of `/lib`, whose functions would come back here. It
/// sources what the legacy `/libexec/zfunc` sourced, except the library of
/// the `fzf` menu (`zfsbootmenu-ui.sh`), which Rust images do not have; no
/// function of the library calls the menu's. The functions that open a
/// shell, and the one that names a filesystem's directory in the state,
/// are `zbm`'s; the legacy one of the latter answers where `zbm` prints
/// nothing.
const LEGACY_FUNCTION: &str = r#"legacy="${1}"
shift
source() {
  case "${1}" in
    /lib/zfsbootmenu-core.sh|/lib/kmsg-log-lib.sh|/lib/zfsbootmenu-kcl.sh)
      builtin source "${legacy}${1}" ;;
    *) builtin source "$@" ;;
  esac
}
for src in /lib/profiling-lib.sh /lib/kmsg-log-lib.sh /lib/zfsbootmenu-core.sh; do
  if ! source "${src}" >/dev/null 2>&1; then
    echo "<3>ZFSBootMenu: unable to source '${src}' in $0" > /dev/kmsg
    exit 1
  fi
done
unset src legacy
# The recovery shell and the chroot go through the gate of zbm, whichever
# function opens them (ADR 0010).
emergency_shell() { /libexec/zbm zfunc emergency_shell "$@"; }
zfs_chroot() { /libexec/zbm zfunc zfs_chroot "$@"; }
# A filesystem's state is where zbm keeps it, so that the legacy functions
# mount it and write its files there too (SB19); where zbm prints nothing,
# the legacy function answers, so that the path is never empty.
eval "zbm_legacy_$(declare -f be_location)"
be_location() {
  local loc
  if [ -n "${1}" ] && loc="$( /libexec/zbm zfunc be_location "$@" )" && [ -n "${loc}" ]; then
    echo "${loc}"
  else
    zbm_legacy_be_location "$@"
  fi
}
func="${1}"
shift
zdebug "Calling ${func} with $*"
"${func}" "$@"
"#;

/// The command that runs the legacy function `function` with `args`, on
/// Bash, with the legacy files below `legacy`, this process's streams and
/// environment, and `env` added (ADR 0017, point 10). `zbm zfunc` replaces
/// itself with it, so that Bash gets the caller's signals as the function
/// did in the caller's shell.
pub fn legacy_function(
    legacy: &Path,
    function: &str,
    args: &[String],
    env: &[(&str, &str)],
) -> Command {
    let command = Command::new("bash")
        .args(["-c", LEGACY_FUNCTION, "/libexec/zfunc"])
        .arg(legacy)
        .arg(function)
        .args(args)
        .stdin(Input::Inherit)
        .stdout(Stream::Inherit)
        .stderr(Stream::Inherit);
    env.iter()
        .fold(command, |command, (name, value)| command.env(name, value))
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

/// Copies `from` to `to`, which does not exist, as `cp -a -- FROM TO`
/// does: a directory with what it holds, a symbolic link as a link, each
/// with its owner, mode and times. GNU's, uutils' and busybox's `cp` take
/// these options. Its errors are captured.
///
/// # Errors
///
/// The error that kept `cp` from starting.
pub fn copy_preserving(runner: &dyn Runner, from: &Path, to: &Path) -> io::Result<Output> {
    let command = Command::new("cp")
        .args(["-a", "--"])
        .arg(from)
        .arg(to)
        .stdin(Input::Null)
        .stdout(Stream::Null)
        .stderr(Stream::Capture);
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
mod tests;
