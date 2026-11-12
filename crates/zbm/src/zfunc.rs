//! `zbm zfunc FUNCTION [ARGS]`: a function of the shell library of the
//! hook contract (ADR 0017, point 10), which the generated library's
//! wrappers and `/libexec/zfunc` run: `zbm_startup::zfunc` runs it.

use std::os::unix::process::CommandExt as _;
use std::path::Path;

use zbm_config::conf::{self, Conf};
use zbm_platform::console::NoConsole;
use zbm_platform::{Command, RealSystem};
use zbm_startup::zfunc::Outcome;

use crate::Io;

/// `zbm zfunc FUNCTION [ARGS]`: runs the function on this process's
/// terminal, if it has one, with the image's configuration.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let Some((function, args)) = args.split_first() else {
        let _ = io.stderr.write_all(crate::table::ZFUNC.usage.as_bytes());
        return crate::USAGE;
    };
    let real = RealSystem::new();
    let terminal = real.terminal().ok();
    let system = match &terminal {
        Some(terminal) => real.with_console(terminal),
        None => real.with_console(&NoConsole),
    };
    let conf = system
        .files
        .read_to_string(Path::new(conf::PATH))
        .map(|text| Conf::parse(&text))
        .unwrap_or_default();
    match zbm_startup::zfunc::run(&system, &conf, function, args, io.stdout) {
        Outcome::Status(status) => status,
        Outcome::Legacy(command) => replace_with(&command, io),
    }
}

/// Replaces this process with `command`, on its streams, so that the
/// legacy function gets the signals of its caller; 127 when it cannot.
#[allow(
    clippy::disallowed_methods,
    reason = "the legacy function replaces this process (`exec`), which the Runner does not do"
)]
fn replace_with(command: &Command, io: &mut Io) -> i32 {
    let mut exec = std::process::Command::new(command.program());
    exec.args(command.get_args());
    for (name, value) in command.get_env() {
        match value {
            Some(value) => exec.env(name, value),
            None => exec.env_remove(name),
        };
    }
    let error = exec.exec();
    let _ = writeln!(io.stderr, "zfunc: unable to run bash: {error}");
    127
}

#[cfg(test)]
mod tests {
    use zbm_host::image::library::{COPIED, FUNCTIONS};

    use super::*;

    /// Runs `body` with an `Io` on buffers; its result and what it wrote
    /// on standard error.
    fn with_io<T>(body: impl FnOnce(&mut Io) -> T) -> (T, String) {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let result = body(&mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        });
        (result, String::from_utf8(stderr).unwrap())
    }

    #[test]
    fn the_applet_runs_a_function_or_says_how() {
        let (status, stderr) = with_io(|io| run(&[], io));
        assert_eq!(status, crate::USAGE);
        assert!(stderr.contains("Usage: zbm zfunc FUNCTION"), "{stderr}");
        // A function with Rust code runs here: an empty pool is not
        // restricted, whatever the machine.
        let (status, _) = with_io(|io| run(&["is_degraded".to_owned(), String::new()], io));
        assert_eq!(status, 1);
    }

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "a command that cannot start, for the exec to fail"
    )]
    fn a_legacy_function_that_cannot_start_is_said() {
        let command = Command::new("/nonexistent/zbm-zfunc-test").arg("x");
        let (status, stderr) = with_io(|io| replace_with(&command, io));
        assert_eq!(status, 127);
        assert!(stderr.contains("zfunc: unable to run bash"), "{stderr}");
    }

    /// ADR 0017, point 10: the functions of the library that still run the
    /// legacy code, which SB14 makes Rust; every function is copied
    /// shell code, Rust or legacy.
    #[test]
    fn the_functions_still_legacy() {
        let natives = zbm_startup::zfunc::natives();
        let legacy: Vec<&str> = FUNCTIONS
            .iter()
            .copied()
            .filter(|name| !COPIED.contains(name) && !natives.contains(name))
            .collect();
        assert_eq!(
            legacy,
            [
                "write_hostid",
                "get_spl_hostid",
                "match_hostid",
                "log_unimportable",
                "check_for_pools",
                "mount_block",
                "mount_zfs",
                "kexec_kernel",
                "duplicate_snapshot",
                "clone_snapshot",
                "create_snapshot",
                "rollback_snapshot",
                "set_default_kernel",
                "set_default_env",
                "find_be_initramfs",
                "find_be_kernels",
                "select_kernel",
                "find_root_prefix",
                "validate_cmdline_cache",
                "kernel_kcl_cache",
                "load_be_cmdline",
                "import_pool",
                "export_pool",
                "rewind_checkpoint",
                "has_resume_device",
                "timed_prompt",
                "resume_prompt",
                "is_writable",
                "set_rw_pool",
                "set_ro_pool",
                "be_has_encroot",
                "be_is_locked",
                "be_keysource",
                "cache_key",
                "load_key",
                "be_location",
                "import_zbm_hooks",
                "is_mountpoint",
                "is_mounted",
                "is_efi_system",
                "mount_efivarfs",
                "is_zfs_filesystem",
                "recursive_umount",
                "print_kmsg_logs",
                "read_kcl_prop",
                "read_kcl_value",
                "_build_zbm_kcl",
                "get_zbm_kcl",
                "make_cmdline_dir",
                "get_zbm_arg",
                "get_zbm_bool",
            ]
        );
        for name in natives {
            assert!(
                FUNCTIONS.contains(&name) && !COPIED.contains(&name),
                "{name}"
            );
        }
    }
}
