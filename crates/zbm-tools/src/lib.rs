//! The system programs ZFSBootMenu runs, other than ZFS (`zbm-zfs`) and
//! encryption (`zbm-crypt`): each function here builds one command line and
//! runs it through a [`Runner`](zbm_platform::Runner), so that the rest of
//! the code describes what it wants and never builds a `Command`.
//!
//! The functions keep the command lines and the streams of the legacy code:
//! where a program's output goes (to the console, nowhere, or back to the
//! caller) is part of what each one does. Most return the
//! [`Output`](zbm_platform::Output) of the run, or the error that kept the
//! program from starting, for the caller to judge.
//!
//! | Module | Programs |
//! | --- | --- |
//! | [`console`] | `tput`: the plain console's prompts |
//! | [`devices`] | `blkid`, `lsblk` |
//! | [`efi`] | `objcopy`, `objdump` |
//! | [`kexec`] | `kexec` |
//! | [`modules`] | `modprobe`, `modinfo`, `insmod` |
//! | [`mount`] | `mount`, `umount`, `mountpoint` |
//! | [`programs`] | hooks, key scripts, editors, shells, Bash's `=~`, `cp` |
//! | [`terminal`] | `tput`, `stty` |
//! | [`udev`] | `udevadm` |

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]

pub mod console;
pub mod devices;
pub mod efi;
pub mod kexec;
pub mod modules;
pub mod mount;
pub mod programs;
pub mod terminal;
pub mod udev;

/// Where a program's output goes when the caller does not read it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shown {
    /// To the console, as the legacy code lets it.
    Console,
    /// Nowhere (`>/dev/null 2>&1`).
    Hidden,
}
