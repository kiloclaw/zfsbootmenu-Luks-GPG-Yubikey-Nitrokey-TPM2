//! `zbm zfunc FUNCTION [ARGS]`: a function of the shell library of the
//! hook contract (ADR 0017, point 10), which the generated library's
//! wrappers and `/libexec/zfunc` run: `zbm_startup::zfunc` runs it.

use std::path::Path;

use zbm_config::conf::{self, Conf};
use zbm_platform::RealSystem;
use zbm_platform::console::NoConsole;

use crate::Io;

/// `zbm zfunc FUNCTION [ARGS]`: runs the function on this process's
/// terminal, if it has one, with the image's configuration.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let Some((function, args)) = args.split_first() else {
        let _ = writeln!(io.stderr, "Usage: zbm zfunc FUNCTION [ARGS]");
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
    zbm_startup::zfunc::run(&system, &conf, function, args)
}

#[cfg(test)]
mod tests {
    use zbm_host::image::library::{COPIED, FUNCTIONS};

    /// ADR 0017, point 10: the functions of the library that still run the
    /// legacy code, which SB6 and SB7 make Rust; every function is copied
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
                "zfs_chroot",
                "zreport",
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
                "zbmcmdline",
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
