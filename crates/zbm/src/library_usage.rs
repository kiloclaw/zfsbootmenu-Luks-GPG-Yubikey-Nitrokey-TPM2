//! The six functions of the shell library that the recovery shell offers as
//! commands, beside `zbm`'s links (`docs/online/recovery-shell.rst`): their
//! usage and summary, which `zbm help commands` and the man page's "The
//! Recovery Shell" list after the applets' (ADR 0018, point 14). The
//! functions are the library's; only their text is here.

/// A function of the shell library that the recovery shell offers.
pub(crate) struct Function {
    /// Its name, as the shell calls it.
    pub(crate) name: &'static str,
    /// What it does, as a sentence without its full stop.
    pub(crate) summary: &'static str,
    /// Its usage text.
    pub(crate) usage: &'static str,
}

/// The functions, in the order of the recovery shell's page.
pub(crate) const FUNCTIONS: [Function; 6] = [
    Function {
        name: "zreport",
        summary: "list the ZFS modules, the pools and the datasets for a bug report",
        usage: "Usage: zreport\n",
    },
    Function {
        name: "zbmcmdline",
        summary: "print ZFSBootMenu's command line, from /etc/cmdline, /etc/cmdline.d/ and \
                  /proc/cmdline",
        usage: "Usage: zbmcmdline\n",
    },
    Function {
        name: "set_rw_pool",
        summary: "export the pool POOL, then import it read/write",
        usage: "Usage: set_rw_pool POOL\n",
    },
    Function {
        name: "set_ro_pool",
        summary: "export the pool POOL, then import it read-only",
        usage: "Usage: set_ro_pool POOL\n",
    },
    Function {
        name: "mount_zfs",
        summary: "mount the filesystem FILESYSTEM at a place of its own and print the mount point",
        usage: "Usage: mount_zfs FILESYSTEM\n",
    },
    Function {
        name: "mount_efivarfs",
        summary: "mount efivarfs at DIRECTORY, /sys/firmware/efi/efivars by default, or mount \
                  it again, read/write (rw) or read-only (ro, the default)",
        usage: "Usage: mount_efivarfs [rw|ro [DIRECTORY]]\n",
    },
];
