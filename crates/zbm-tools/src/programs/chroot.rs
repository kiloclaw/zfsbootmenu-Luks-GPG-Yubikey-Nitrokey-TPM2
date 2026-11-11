//! A shell in a chroot of a filesystem.

use std::io;
use std::path::{Path, PathBuf};

use zbm_platform::{Command, Files, Input, Runner, Stream};
use zbm_types::FilesystemName;

/// The shells a chroot tries, in order, as `zfs-chroot` tried them: the
/// program, the arguments it is tried with before `-c "exit 0"`, and those
/// it runs with.
const SHELLS: [(&str, &[&str], &[&str]); 3] = [
    ("/bin/bash", &[], &["--norc"]),
    ("/bin/sh", &[], &[]),
    ("/bin/busybox", &["sh"], &["sh"]),
];

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

impl Chroot {
    /// The chroot of `name` mounted on `root`, with the first shell that
    /// runs there, as `zfs-chroot` found it: `/bin/bash`, `/bin/sh`, then
    /// `/bin/busybox sh`, each where `files` finds it executable below
    /// `root` and where it ends well with `-c "exit 0"` run in the chroot,
    /// quietly; `None` when none does.
    ///
    /// # Errors
    ///
    /// The error that kept a shell from starting in the chroot, such as a
    /// root that cannot be entered: no other shell is tried then.
    pub fn find(
        runner: &dyn Runner,
        files: &dyn Files,
        root: &Path,
        name: &FilesystemName,
    ) -> io::Result<Option<Chroot>> {
        for (shell, tried, args) in SHELLS {
            let inside = root.join(shell.trim_start_matches('/'));
            if !files
                .metadata(&inside)
                .is_ok_and(|metadata| metadata.is_executable())
            {
                continue;
            }
            let probe = Command::new(shell)
                .args(tried.iter().copied())
                .args(["-c", "exit 0"])
                .root(root)
                .stdin(Input::Null)
                .stdout(Stream::Null)
                .stderr(Stream::Null);
            if runner.run(&probe)?.status.is_success() {
                return Ok(Some(Chroot {
                    root: root.to_path_buf(),
                    shell: PathBuf::from(shell),
                    args: args.iter().map(|&arg| arg.to_owned()).collect(),
                    name: name.clone(),
                }));
            }
        }
        Ok(None)
    }

    /// The command of the chroot's shell: in its root, with a prompt that
    /// names the filesystem in yellow and the directory, as `zfs-chroot`'s
    /// did, and without `PROMPT_COMMAND`, which would name a program the
    /// chroot may not have.
    pub(super) fn command(&self) -> Command {
        let ps1 = format!("\\[\\033[0;33m\\]{}\\[\\033[0m\\] \\w > ", self.name);
        Command::new(&self.shell)
            .args(&self.args)
            .root(&self.root)
            .env("PS1", ps1)
            .env_remove("PROMPT_COMMAND")
    }
}

#[cfg(test)]
mod tests;
