//! A shell in a chroot of a filesystem.

use std::path::PathBuf;

use zbm_types::FilesystemName;

/// A chroot of a filesystem, mounted and ready, and the shell that runs in
/// it: the program that ran in it already, with the arguments it takes
/// there (`/bin/bash --norc`, `/bin/sh`, or `/bin/busybox sh`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chroot {
    /// Where the filesystem is mounted, with the kernel's file systems
    /// below it: the shell's root.
    pub root: PathBuf,
    /// The shell, as a path in the chroot.
    pub shell: PathBuf,
    /// The shell's arguments.
    pub args: Vec<String>,
    /// The filesystem, which the shell's prompt names.
    pub name: FilesystemName,
}
