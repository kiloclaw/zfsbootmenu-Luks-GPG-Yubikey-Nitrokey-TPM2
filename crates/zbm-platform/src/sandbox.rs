//! The sandbox profiles of ZFSBootMenu's processes (ROADMAP.md, SE11; ADR
//! 0002): what each gives up, as a [`Profile`]. The trait is sealed: a
//! profile is one of those here, so that a review reads every one in one
//! place. `zbm_sys::sandbox` applies them: [`Profile::confine`] to this
//! process once it opened what it needs, [`Sandboxed::sandboxed`] to a
//! program before it starts.

use std::io;
use std::path::PathBuf;
use std::process::Command;

use zbm_sys::sandbox::{self, Access, Applied, Capability, Confinement};

mod sealed {
    /// Only this module's profiles are profiles.
    pub trait Sealed {}
}

/// What a process of ZFSBootMenu gives up.
pub trait Profile: sealed::Sealed {
    /// Its name, for the log.
    fn name(&self) -> &'static str;

    /// What it gives up.
    fn confinement(&self) -> Confinement;

    /// Applies it to this process, every thread.
    ///
    /// # Errors
    ///
    /// When a step fails: the process is then confined by the steps before
    /// it, which are never undone.
    fn confine(&self) -> io::Result<Applied> {
        sandbox::confine(&self.confinement())
    }
}

/// Starting programs confined.
pub trait Sandboxed {
    /// Has the program confine itself to `profile` before it runs.
    ///
    /// # Errors
    ///
    /// When `profile` cannot be applied before a program runs: one that
    /// confines files.
    fn sandboxed(&mut self, profile: &dyn Profile) -> io::Result<&mut Self>;
}

impl Sandboxed for Command {
    fn sandboxed(&mut self, profile: &dyn Profile) -> io::Result<&mut Self> {
        sandbox::prepare(self, &profile.confinement())?;
        Ok(self)
    }
}

/// The keyholder (zone Z1, `zbm unlock serve`) and the programs it runs:
/// `cryptsetup`, the TPM2 and FIDO2 tools, `gpg`, `clevis`. They stay
/// root, for the device mapper and the TPM, and read key files wherever a
/// key device is mounted, so they give up gaining privileges alone: no
/// program they start becomes another user or gains capabilities by its
/// file (`no_new_privs`). A filter of their calls waits for the key
/// methods in Rust (CD6 to CD10), which call no programs.
#[derive(Clone, Copy, Debug, Default)]
pub struct Keyholder;

impl sealed::Sealed for Keyholder {}

impl Profile for Keyholder {
    fn name(&self) -> &'static str {
        "keyholder"
    }

    fn confinement(&self) -> Confinement {
        Confinement::default()
    }
}

/// The console on the display (zone Z3, `zbm console`), from before it
/// starts its threads. It stays user 0, as it opens the display, the
/// virtual terminal and the keyboards again when it takes the display
/// back after a failed `kexec`, and writes the state files the boot reads:
/// so it keeps two capabilities, the virtual terminal's modes and its
/// locked memory; reaches the devices, `/sys`, `/proc`, its fonts and
/// layouts, the state directory and the sockets' directory alone; runs
/// nothing; and makes the calls of [`CONSOLE_CALLS`] alone.
#[derive(Clone, Debug)]
pub struct Console {
    /// The state directory, `/zfsbootmenu`.
    pub state: PathBuf,
    /// The directory of the sockets, `/run/zfsbootmenu`.
    pub sockets: PathBuf,
    /// The image's fonts and layouts, `/usr/share/zfsbootmenu`.
    pub share: PathBuf,
    /// Where test builds mirror the screen's text, if anywhere.
    pub mirror: Option<PathBuf>,
    /// The serial console it serves too, if any (ROADMAP.md UI9).
    pub serial: Option<PathBuf>,
}

/// The calls of the console: its loop (`ppoll`, `read`, `write`, `ioctl`
/// on the display, the terminal and the keyboards), its files and their
/// directories, inotify on `/dev/input`, the control socket and its
/// threads, the display's buffers and its time. Not `execve`, `kill`,
/// `ptrace` or a user change; `prctl` only to name a thread.
pub const CONSOLE_CALLS: &[&str] = &[
    // Reading, writing and waiting.
    "read",
    "write",
    "readv",
    "writev",
    "pread64",
    "pwrite64",
    "lseek",
    "ioctl",
    "fcntl",
    "close",
    "close_range",
    "dup",
    "dup3",
    "pipe2",
    "poll",
    "ppoll",
    "pselect6",
    "epoll_create1",
    "epoll_ctl",
    "epoll_wait",
    "epoll_pwait",
    "epoll_pwait2",
    "eventfd2",
    // Files and directories.
    "openat",
    "fstat",
    "newfstatat",
    "statx",
    "fstatfs",
    "faccessat2",
    "getdents64",
    "readlink",
    "readlinkat",
    "getcwd",
    "ftruncate",
    "fsync",
    "mkdir",
    "mkdirat",
    "unlink",
    "unlinkat",
    "rename",
    "renameat",
    "renameat2",
    "chmod",
    "fchmod",
    "fchmodat",
    "inotify_init1",
    "inotify_add_watch",
    "inotify_rm_watch",
    // The control socket and the client channel.
    "socket",
    "bind",
    "listen",
    "accept4",
    "connect",
    "sendto",
    "recvfrom",
    "sendmsg",
    "recvmsg",
    "getsockopt",
    "setsockopt",
    "getsockname",
    "getpeername",
    "shutdown",
    // Memory: the heap, thread stacks, the display's buffers, locked.
    "brk",
    "mmap",
    "munmap",
    "mremap",
    "mprotect",
    "madvise",
    "mlock",
    "munlock",
    "mlockall",
    "membarrier",
    // Threads, signals and time.
    "clone",
    "clone3",
    "set_robust_list",
    "rseq",
    "set_tid_address",
    "futex",
    "futex_waitv",
    "sched_yield",
    "sched_getaffinity",
    "rt_sigaction",
    "rt_sigprocmask",
    "rt_sigreturn",
    "sigaltstack",
    "restart_syscall",
    "nanosleep",
    "clock_nanosleep",
    "clock_gettime",
    "clock_getres",
    "gettimeofday",
    "exit",
    "exit_group",
    // What the process is.
    "getpid",
    "getppid",
    "gettid",
    "getuid",
    "geteuid",
    "getgid",
    "getegid",
    "getresuid",
    "getresgid",
    "uname",
    "sysinfo",
    "prlimit64",
    "getrandom",
    "prctl/15",
    "fdatasync",
];

impl Console {
    /// The paths it reaches, and how.
    fn files(&self) -> Vec<(PathBuf, Access)> {
        let mut files = vec![
            (PathBuf::from("/dev/dri"), Access::Device),
            (PathBuf::from("/dev/input"), Access::Device),
            (PathBuf::from("/dev/kmsg"), Access::ReadWrite),
            (PathBuf::from("/dev/null"), Access::ReadWrite),
            (PathBuf::from("/sys"), Access::Read),
            (PathBuf::from("/proc"), Access::Read),
            (self.share.clone(), Access::Read),
            (self.state.clone(), Access::Full),
            (self.sockets.clone(), Access::Full),
        ];
        // The virtual terminals, each a file: the console takes the one
        // that is shown.
        files.extend(
            (0..64).map(|number| (PathBuf::from(format!("/dev/tty{number}")), Access::Device)),
        );
        // A crash report reads the program it is of.
        if let Ok(program) = std::env::current_exe() {
            files.push((program, Access::Read));
        }
        if let Some(mirror) = &self.mirror {
            files.push(match mirror.parent() {
                Some(parent) if !mirror.exists() => (parent.to_owned(), Access::Full),
                _ => (mirror.clone(), Access::Device),
            });
        }
        if let Some(serial) = &self.serial {
            files.push((serial.clone(), Access::Device));
        }
        files
    }
}

impl sealed::Sealed for Console {}

impl Profile for Console {
    fn name(&self) -> &'static str {
        "console"
    }

    fn confinement(&self) -> Confinement {
        Confinement {
            user: None,
            files: Some(self.files()),
            calls: Some(CONSOLE_CALLS),
            capabilities: Some(vec![Capability::SysTtyConfig, Capability::IpcLock]),
        }
    }
}

/// The menu as a client of the session (`zfsbootmenu` over SSH, ROADMAP.md
/// PM10; zone Z4), once it attached: the session performs its actions, so
/// it keeps no capability, runs nothing and starts no thread. It stays user
/// 0, the owner of the kernel log and the state directory, which its log
/// and a crash report write; reaches its terminal, the directory of the
/// sockets, where it tells the console of a secret prompt, and its program,
/// which a crash report reads; and makes the calls of [`MENU_CALLS`] alone.
#[derive(Clone, Debug)]
pub struct Menu {
    /// The state directory, `/zfsbootmenu`.
    pub state: PathBuf,
    /// The directory of the sockets, `/run/zfsbootmenu`.
    pub sockets: PathBuf,
}

/// The calls of the menu over SSH: its loop (`ppoll`, `read`, `write`, the
/// terminal's `ioctl`), the session's socket and the console's, its log,
/// its memory and its time. Not `clone`, `execve`, `kill`, `ptrace`,
/// `prctl` or a user change.
pub const MENU_CALLS: &[&str] = &[
    // Reading, writing and waiting.
    "read",
    "write",
    "readv",
    "writev",
    "lseek",
    "ioctl",
    "fcntl",
    "close",
    "dup",
    "dup3",
    "poll",
    "ppoll",
    "pselect6",
    // Its log, a crash report, and the terminal it opens again.
    "openat",
    "fstat",
    "newfstatat",
    "statx",
    "faccessat2",
    "readlink",
    "readlinkat",
    "getcwd",
    "fsync",
    "fdatasync",
    "rename",
    "renameat",
    "renameat2",
    "unlink",
    "unlinkat",
    // The session's socket and the console's.
    "socket",
    "connect",
    "sendto",
    "recvfrom",
    "sendmsg",
    "recvmsg",
    "getsockopt",
    "setsockopt",
    "shutdown",
    // Memory, a secret's locked.
    "brk",
    "mmap",
    "munmap",
    "mremap",
    "mprotect",
    "madvise",
    "mlock",
    "munlock",
    // Signals and time.
    "futex",
    "rt_sigaction",
    "rt_sigprocmask",
    "rt_sigreturn",
    "sigaltstack",
    "restart_syscall",
    "nanosleep",
    "clock_nanosleep",
    "clock_gettime",
    "clock_getres",
    "gettimeofday",
    "exit",
    "exit_group",
    // What the process is.
    "getpid",
    "gettid",
    "getuid",
    "geteuid",
    "getgid",
    "getegid",
    "uname",
    "prlimit64",
    "getrandom",
];

impl Menu {
    /// The paths it reaches, and how.
    fn files(&self) -> Vec<(PathBuf, Access)> {
        let mut files = vec![
            // Its terminal, as `/dev/tty` opens it again.
            (PathBuf::from("/dev/tty"), Access::Device),
            (PathBuf::from("/dev/kmsg"), Access::ReadWrite),
            (PathBuf::from("/dev/null"), Access::ReadWrite),
            (self.state.clone(), Access::Full),
            (self.sockets.clone(), Access::ReadWrite),
        ];
        if let Ok(program) = std::env::current_exe() {
            files.push((program, Access::Read));
        }
        files
    }
}

impl sealed::Sealed for Menu {}

impl Profile for Menu {
    fn name(&self) -> &'static str {
        "menu"
    }

    fn confinement(&self) -> Confinement {
        Confinement {
            user: None,
            files: Some(self.files()),
            calls: Some(MENU_CALLS),
            capabilities: Some(Vec::new()),
        }
    }
}

/// Says what `applied` confined, for the log: `no new privileges,
/// seccomp, landlock 7`.
pub fn describe(applied: &Applied) -> String {
    let mut parts = vec!["no new privileges"];
    let landlock;
    if applied.user {
        parts.push("another user");
    }
    if applied.seccomp {
        parts.push("seccomp");
    }
    match applied.landlock {
        Some(abi) => {
            landlock = format!("landlock {abi}");
            parts.push(&landlock);
        }
        None => parts.push("no landlock"),
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests;
