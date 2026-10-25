//! The commands of the Phase 5 runtime: the start-up hooks and the start-up
//! program. Each builds the real system and calls `zbm-boot`.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use zbm_config::conf::Conf;
use zbm_platform::{RealSystem, System, TracedRunner};
use zbm_startup::autosize::{self, Outcome};
use zbm_startup::preinit;
use zbm_startup::program::{Modes, Startup, is_on};
use zbm_startup::step::Rerun;
use zbm_sys::Signal;

use crate::Io;
use crate::supervisor::{Program, Supervisor};

/// Where the boot image installs `zbm`.
const ZBM: &str = "/libexec/zbm";

/// This program, as the boot image installs it.
fn zbm() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from(ZBM))
}

/// What the legacy scripts do when they cannot read their configuration:
/// warn and start a shell.
fn give_up(io: &mut Io, what: &str) -> i32 {
    let _ = writeln!(
        io.stderr,
        "\x1b[0;31mWARNING: {what} was not sourced; unable to proceed\x1b[0m"
    );
    let error = std::process::Command::new("/bin/bash").exec();
    let _ = writeln!(io.stderr, "zbm: unable to start /bin/bash: {error}");
    1
}

/// `zfsbootmenu-parse-commandline`, for dracut's `cmdline` hook and
/// mkinitcpio.
pub fn parse_commandline(_args: &[String], io: &mut Io) -> i32 {
    let real = RealSystem::new();
    let terminal = match real.terminal() {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm: no terminal: {error}");
            return 1;
        }
    };
    let system = real.with_console(&terminal);
    match preinit::parse_commandline(&system, Path::new("/")) {
        Ok(_) => 0,
        Err(_) => give_up(io, zbm_config::conf::PATH),
    }
}

/// `zfsbootmenu-preinit`, for dracut's `pre-mount` hook and mkinitcpio: sets up
/// the console, then replaces this process with the supervisor in a new
/// session on the control terminal. The supervisor starts with the variables
/// of `/etc/zfsbootmenu.conf` and `/etc/profile` exported, as the legacy
/// start-up program sources them, so every program it starts sees them.
pub fn preinit(_args: &[String], io: &mut Io) -> i32 {
    let real = RealSystem::new();
    let terminal = match real.terminal() {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm: no terminal: {error}");
            return 1;
        }
    };
    let system = real.with_console(&terminal);
    let Ok(conf) = preinit::read_conf(&system, Path::new("/")) else {
        return give_up(io, zbm_config::conf::PATH);
    };
    let options = preinit::saved_options(&system, Path::new("/"), &conf);
    preinit::prepare(&system, Path::new("/"), &options);

    // https://busybox.net/FAQ.html#job_control: the shell opens the terminal
    // after `setsid`, so it becomes the session's controlling terminal.
    let script = "tty=$1; shift; source /etc/zfsbootmenu.conf; source /etc/profile; \
                  exec \"$@\" <\"${tty}\" >\"${tty}\" 2>&1";
    let error = std::process::Command::new("setsid")
        .args(["bash", "-c", script, "zfsbootmenu-preinit"])
        .arg(&options.control_term)
        .arg(zbm())
        .args(["zfsbootmenu-init", "--supervise"])
        .exec();
    let _ = writeln!(io.stderr, "zbm: unable to start the supervisor: {error}");
    1
}

/// The console on the display that the command line asks for
/// (`zbm.console=drm`), as a program for the supervisor.
fn display_console() -> Option<Program> {
    let real = RealSystem::new();
    let terminal = real.terminal().ok()?;
    let system = real.with_console(&terminal);
    let conf = preinit::read_conf(&system, Path::new("/")).ok()?;
    let args = zbm_startup::console::from_image(&system, Path::new("/"), &conf)?;
    Some(Program { path: zbm(), args })
}

/// `zfsbootmenu-init`: the start-up program, or with `--supervise` the
/// supervisor that runs it as process 1.
///
/// Options: `--supervise`; `--console` (what `ZFSBOOTMENU_CONSOLE` does, which
/// the supervisor passes as an argument so that no program the start-up
/// program starts inherits it); `--resume`.
pub fn init(args: &[String], io: &mut Io) -> i32 {
    let mut modes = Modes::default();
    for arg in args {
        match arg.as_str() {
            "--supervise" => {
                let mut supervisor = Supervisor::new(zbm());
                supervisor.console = display_console();
                supervisor.run(None, &mut |_| {});
                return 0;
            }
            "--console" => modes.console = true,
            "--resume" => modes.resume = true,
            other => {
                let _ = writeln!(io.stderr, "zfsbootmenu-init: unknown option {other}");
                return crate::USAGE;
            }
        }
    }
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    modes.console |= is_on(&var("ZFSBOOTMENU_CONSOLE"));
    if is_on(&var("ZFSBOOTMENU_FORCE_INIT")) {
        modes.rerun = Rerun::All;
    }

    // trap '' SIGINT; and Bash, which ran it, ignores SIGQUIT (Ctrl-\\)
    // while the programs it starts get the default action
    let _ = zbm_sys::ignore_signal(Signal::Interrupt);
    let _ = zbm_sys::discard_signal(Signal::Quit);
    on_console(io, |system, conf, _| {
        let startup = Startup::new(system, conf);
        // On the console at boot, the stages run in the menu's start-up
        // view when the terminal can run it; resumed, they are done.
        if modes.console
            && !modes.resume
            && let Some(status) = crate::startup::run_in_menu(&startup, system, conf, modes)
        {
            return status;
        }
        startup.run(modes, None)
    })
}

/// Runs `program` on the real system with the control terminal as its
/// console and the image's configuration, as `zfsbootmenu-init` and the
/// menu run; every program it runs goes to the journal (`zbm debug
/// commands`). It gets `io` back, for its messages.
pub(crate) fn on_console(
    io: &mut Io,
    program: impl FnOnce(System<'_>, &Conf, &mut Io) -> i32,
) -> i32 {
    let real = RealSystem::new();
    let terminal = match real.terminal() {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm: no terminal: {error}");
            return 1;
        }
    };
    let system = real.with_console(&terminal);
    let Ok(conf) = preinit::read_conf(&system, Path::new("/")) else {
        return give_up(io, zbm_config::conf::PATH);
    };
    let traced = TracedRunner::new(system.runner, system.clock);
    program(system.with_runner(&traced), &conf, io)
}

/// `console-autosize`: the early-setup hook `30-console-autosize` of the Rust
/// runtime, which sizes the console font to the display (`zbm.autosize`).
///
/// Options: `--console DEVICE` sizes this virtual console instead of the
/// control terminal. The exit status is 0 when the font was set or left
/// alone on purpose, 1 when no font could be loaded.
pub fn console_autosize(args: &[String], io: &mut Io) -> i32 {
    let mut console = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match (arg.as_str(), args.next()) {
            ("--console", Some(device)) => console = Some(device.clone()),
            (other, _) => {
                let _ = writeln!(io.stderr, "console-autosize: unknown option {other}");
                return crate::USAGE;
            }
        }
    }
    let real = RealSystem::new();
    let terminal = match real.terminal() {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm: no terminal: {error}");
            return 1;
        }
    };
    let system = real.with_console(&terminal);
    match autosize::hook(&system, Path::new("/"), console) {
        Ok(Outcome::Failed) => 1,
        Ok(Outcome::Set { .. } | Outcome::Skipped(_)) => 0,
        Err(_) => {
            let _ = writeln!(
                io.stderr,
                "ERROR: unable to source {}",
                zbm_config::conf::PATH
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_the_image_s_configuration_there_is_no_console_on_the_display() {
        // The machine of the tests has no /etc/zfsbootmenu.conf.
        assert!(!Path::new(zbm_config::conf::PATH).exists());
        assert_eq!(display_console(), None);
        assert!(zbm().file_name().is_some());
    }
}
