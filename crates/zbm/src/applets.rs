//! The commands of the Phase 5 runtime: the start-up hooks and the start-up
//! program. Each builds the real system and calls `zbm-boot`.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use zbm_platform::RealSystem;
use zbm_startup::preinit;
use zbm_startup::program::{Modes, Startup, is_on};
use zbm_startup::step::Rerun;
use zbm_sys::Signal;

use crate::Io;
use crate::supervisor::Supervisor;

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
        Err(_) => give_up(io, zbm_cmdline::conf::PATH),
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
        return give_up(io, zbm_cmdline::conf::PATH);
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
                Supervisor::new(zbm()).run(None, &mut |_| {});
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

    // trap '' SIGINT
    let _ = zbm_sys::ignore_signal(Signal::Interrupt);
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
        return give_up(io, zbm_cmdline::conf::PATH);
    };
    Startup::new(system, &conf).run(modes, None)
}
