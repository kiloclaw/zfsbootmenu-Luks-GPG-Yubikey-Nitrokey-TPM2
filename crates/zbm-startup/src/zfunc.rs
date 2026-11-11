//! The functions of the shell library of the hook contract (ADR 0017,
//! point 10), which `zbm zfunc` runs for the generated library's wrappers
//! and `/libexec/zfunc`. A function with Rust code runs it; any other runs
//! as the legacy runtime ran it, from the legacy files that Rust images
//! keep below [`LEGACY_LIBRARY`], on Bash, with the caller's streams; the
//! exit status is the function's.
//!
//! The emergency shell and the chroot go through the gate of the recovery
//! shell (ADR 0010): in a locked image they ask for the PIN first. The
//! chroot runs `zfs-chroot` as a child, as the legacy function ran it on
//! Bash, and that asks for the PIN (ADR 0018, point 8).

use std::path::{Path, PathBuf};

use zbm_boot::Recovery as _;
use zbm_config::conf::Conf;
use zbm_platform::console::Screen;
use zbm_platform::{Command, Context, System};
use zbm_state::State;
use zbm_tools::programs::{self, LEGACY_LIBRARY};

use crate::shell::Shell;

/// What a function with Rust code gets: the system, the state directory
/// and the function's arguments; it returns the exit status.
type Native = fn(&System<'_>, &State<'_>, &[String]) -> i32;

/// The functions with Rust code; the others run as the legacy runtime ran
/// them.
const NATIVE: [(&str, Native); 3] = [
    ("emergency_shell", emergency_shell),
    ("is_degraded", is_degraded),
    ("zfs_chroot", zfs_chroot),
];

/// What running a function comes to.
#[derive(Debug)]
pub enum Outcome {
    /// It ran, with this exit status.
    Status(i32),
    /// The legacy function, which the caller runs by replacing its
    /// process with this command.
    Legacy(Command),
}

/// Runs `function` with `args` on `system`, with the image's
/// configuration `conf` where the environment does not set a variable.
pub fn run(system: &System<'_>, conf: &Conf, function: &str, args: &[String]) -> Outcome {
    match native(function) {
        Some(native) => {
            let base = base(system, conf);
            let state = State::new(system.files, &base);
            let context = Context::new(*system);
            let loglevel = conf.get("loglevel").unwrap_or_default();
            let _log = context.kernel_log(loglevel, &base);
            let joined = args.join(" ");
            zbm_trace::debug!("Calling {function} with {joined}"; function, %joined);
            Outcome::Status(native(system, &state, args))
        }
        None => legacy(system, conf, function, args),
    }
}

/// The functions with Rust code, by name.
pub fn natives() -> Vec<&'static str> {
    NATIVE.iter().map(|(name, _)| *name).collect()
}

/// The Rust code of `function`, if it has some.
fn native(function: &str) -> Option<Native> {
    NATIVE
        .iter()
        .find(|(name, _)| *name == function)
        .map(|(_, native)| *native)
}

/// The state directory: `BASE` of the environment, as the legacy
/// functions read it, else of the configuration.
fn base(system: &System<'_>, conf: &Conf) -> PathBuf {
    system
        .environment
        .non_empty("BASE")
        .map_or_else(|| crate::preinit::base(conf), PathBuf::from)
}

/// The command of the legacy `function` with `args`, with the variables
/// of the configuration the environment does not set, as the legacy
/// `zfunc` sourced the file; status 1 when the legacy files are missing.
fn legacy(system: &System<'_>, conf: &Conf, function: &str, args: &[String]) -> Outcome {
    let legacy = Path::new(LEGACY_LIBRARY);
    if !system.files.exists(&legacy.join("lib/zfsbootmenu-core.sh")) {
        zbm_trace::error!(
            "zfunc: {function}: the legacy library is not in {legacy}";
            function, legacy = LEGACY_LIBRARY
        );
        return Outcome::Status(1);
    }
    let env: Vec<(&str, &str)> = conf
        .vars()
        .iter()
        .filter(|(name, _)| system.environment.var_os(name).is_none())
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    Outcome::Legacy(programs::legacy_function(legacy, function, args, &env))
}

/// The emergency shell, with an optional message: the recovery shell of
/// the start-up program, behind the lock of the image.
fn emergency_shell(system: &System<'_>, state: &State<'_>, args: &[String]) -> i32 {
    open_shell(&Shell::new(*system, state), args)
}

/// Opens `shell` with the message of `args`, as the legacy function does.
fn open_shell(shell: &Shell<'_, '_>, args: &[String]) -> i32 {
    shell.emergency_shell(args.first().map_or("", String::as_str));
    0
}

/// Whether the pool of `args` is restricted: 0 when a line of the state
/// directory's `degraded` is exactly its name, 1 when none is or the name
/// is empty, 2 when the file cannot be read, as `grep` reports.
fn is_degraded(system: &System<'_>, state: &State<'_>, args: &[String]) -> i32 {
    let pool = args.first().map_or("", String::as_str);
    if pool.is_empty() {
        return 1;
    }
    if system.files.read(&state.base().join("degraded")).is_err() {
        return 2;
    }
    i32::from(!state.is_degraded(pool))
}

/// A shell in a chroot of the filesystem of `args`, as the legacy
/// function opened one: on a clear screen with the cursor shown,
/// `zfs-chroot` with the name as one argument, on the console; its exit
/// status. Without a filesystem nothing runs, and the status is 1.
fn zfs_chroot(system: &System<'_>, _state: &State<'_>, args: &[String]) -> i32 {
    let fs = args.first().map_or("", String::as_str);
    if fs.is_empty() {
        zbm_trace::error!("fs is undefined");
        return 1;
    }
    zbm_trace::debug!("fs set to {fs}"; fs);
    system.console.screen(Screen::Clear);
    system.console.screen(Screen::ShowCursor);
    zbm_trace::debug!("chroot environment: {fs}"; fs);
    match programs::applet(system.runner, programs::ZBM, &["zfs-chroot", fs], &[]) {
        Ok(output) => output.status.shell_code(),
        Err(error) => {
            zbm_trace::error!("unable to run zfs-chroot: {error}"; %error);
            NOT_STARTED
        }
    }
}

/// The status of a program that could not start, as a shell reports it.
const NOT_STARTED: i32 = 127;

#[cfg(test)]
mod tests;
