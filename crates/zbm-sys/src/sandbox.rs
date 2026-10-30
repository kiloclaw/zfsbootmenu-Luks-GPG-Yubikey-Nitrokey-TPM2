//! What a process gives up once it opened what it needs (ROADMAP.md, SE11;
//! ADR 0002, zone Z3): its user, the right to gain privileges, the files
//! it may still reach, and the system calls it may still make.
//!
//! [`Confinement`] describes it; [`confine`] applies it to the whole
//! process, every thread, in this order: the groups and the user (which
//! also drops every capability), `no_new_privs`, the landlock ruleset, and
//! the seccomp filter last, as it may forbid the calls before it.
//!
//! The seccomp filter is an allow-list: a call not on it fails with
//! `EPERM`, and the kernel logs it (`SECCOMP_FILTER_FLAG_LOG`), so that a
//! call a rare path needs shows in the kernel log rather than killing the
//! process; a call of another architecture's ABI kills it. Landlock is
//! best effort: a kernel without it ([`Applied::landlock`] is `None`)
//! leaves the files to the user's permissions alone.
//!
//! `rustix` has no call for seccomp filters, landlock, or a user change
//! that every thread takes (the kernel's `setresuid` changes one thread;
//! glibc's changes them all), so those are glibc calls here.

use std::ffi::{c_int, c_long, c_uint};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use rustix::process::{Gid, Uid};

unsafe extern "C" {
    // `long syscall(long number, ...)`: glibc's way to the calls it has no
    // function for.
    fn syscall(number: c_long, ...) -> c_long;
    // `int setgroups(size_t size, const gid_t *list)`, for every thread.
    fn setgroups(size: usize, list: *const c_uint) -> c_int;
    // `int setresgid(gid_t, gid_t, gid_t)` and `setresuid`, for every thread.
    fn setresgid(real: c_uint, effective: c_uint, saved: c_uint) -> c_int;
    fn setresuid(real: c_uint, effective: c_uint, saved: c_uint) -> c_int;
}

/// What a process may still read, write or use below a path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    /// Read files and list directories.
    Read,
    /// Read, and write files that are there.
    ReadWrite,
    /// Read, write and use devices (`ioctl`), as input devices and
    /// terminals need.
    Device,
    /// Read, write, and make and remove files.
    Full,
}

/// A user and its groups.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct User {
    /// The user ID.
    pub uid: u32,
    /// The group ID.
    pub gid: u32,
    /// The supplementary groups, such as `input` and `video` for devices
    /// that come later.
    pub groups: Vec<u32>,
}

/// What a process gives up.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Confinement {
    /// The user it becomes; `None`: it keeps its own.
    pub user: Option<User>,
    /// The paths it may still reach, with how; `None`: no landlock.
    pub files: Option<Vec<(PathBuf, Access)>>,
    /// The system calls it may still make, by name; `None`: no filter. A
    /// name followed by `/` and a number allows the call only when its
    /// first argument is that number: `prctl/15`, `PR_SET_NAME`.
    pub calls: Option<&'static [&'static str]>,
    /// The capabilities it keeps, when it stays user 0; `None`: all. The
    /// others leave every set, the bounding set too, so that nothing it
    /// runs gets them back. They are the calling thread's: confine before
    /// starting threads.
    pub capabilities: Option<Vec<Capability>>,
}

/// A capability a confined process may keep.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Capability {
    /// `CAP_SYS_TTY_CONFIG`: the virtual terminal's modes (`KDSETMODE`).
    SysTtyConfig,
    /// `CAP_IPC_LOCK`: memory locked beyond `RLIMIT_MEMLOCK`.
    IpcLock,
}

impl Capability {
    /// Its number.
    fn number(self) -> u32 {
        match self {
            Capability::IpcLock => 14,
            Capability::SysTtyConfig => 26,
        }
    }
}

/// What [`confine`] did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Applied {
    /// Whether the user changed.
    pub user: bool,
    /// The landlock ABI version the ruleset used; `None` when no ruleset
    /// was asked for or the kernel has no landlock.
    pub landlock: Option<u32>,
    /// Whether a seccomp filter was installed.
    pub seccomp: bool,
}

/// Applies `confinement` to this process, every thread.
///
/// # Errors
///
/// When a step fails that the kernel has: the steps before it stay done.
pub fn confine(confinement: &Confinement) -> io::Result<Applied> {
    #[cfg(coverage)]
    let profiled = coverage::with_profile(confinement);
    #[cfg(coverage)]
    let confinement = &profiled;
    let mut applied = Applied::default();
    if let Some(user) = &confinement.user {
        become_user(user)?;
        applied.user = true;
    }
    if let Some(kept) = &confinement.capabilities {
        keep_capabilities(kept)?;
    }
    rustix::thread::set_no_new_privs(true)?;
    if let Some(files) = &confinement.files {
        applied.landlock = restrict_files(files)?;
    }
    if let Some(calls) = confinement.calls {
        Filter::allowing(calls).install()?;
        applied.seccomp = true;
    }
    Ok(applied)
}

/// Has the program `command` starts give up what `confinement` says
/// before it runs: its groups and user, `no_new_privs` and the seccomp
/// filter, built here, between `fork` and `exec`, where nothing may be
/// allocated. The filter must allow `execve`.
///
/// # Errors
///
/// `Unsupported` for files: landlock opens paths, which a child may not do
/// before `exec`; the program confines its files itself.
pub fn prepare(command: &mut std::process::Command, confinement: &Confinement) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    if confinement.files.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "a program confines its own files",
        ));
    }
    let user = confinement.user.as_ref().map(|user| {
        let groups: Vec<Gid> = user.groups.iter().map(|gid| Gid::from_raw(*gid)).collect();
        (Uid::from_raw(user.uid), Gid::from_raw(user.gid), groups)
    });
    let filter = confinement.calls.map(Filter::allowing);
    let before_exec = move || {
        // The child has one thread: the kernel's calls, for one thread,
        // change it all.
        if let Some((uid, gid, groups)) = &user {
            rustix::thread::set_thread_groups(groups)?;
            rustix::thread::set_thread_res_gid(*gid, *gid, *gid)?;
            rustix::thread::set_thread_res_uid(*uid, *uid, *uid)?;
        }
        rustix::thread::set_no_new_privs(true)?;
        if let Some(filter) = &filter {
            filter.install()?;
        }
        Ok(())
    };
    // SAFETY: the closure runs between `fork` and `exec`: it makes the
    // calls that change the user, a `prctl` and a `seccomp` call on what
    // was built before, and allocates nothing. `install` builds its program
    // on the stack; its error path, which formats, follows only a thread
    // that kept its filter, and the child has one thread.
    unsafe {
        command.pre_exec(before_exec);
    }
    Ok(())
}

/// `PR_CAPBSET_DROP`, and the last capability the kernel may know.
const PR_CAPBSET_DROP: c_long = 24;
const LAST_CAPABILITY: u32 = 63;

/// The number of `prctl` on this architecture.
const PRCTL: c_long = if cfg!(target_arch = "aarch64") {
    167
} else {
    157
};

/// Keeps the capabilities `kept`, in the calling thread: the others leave
/// the bounding set (which needs `CAP_SETPCAP`, so first), then the
/// effective, permitted and inheritable sets.
fn keep_capabilities(kept: &[Capability]) -> io::Result<()> {
    use rustix::thread::{CapabilitySet, CapabilitySets};
    let kept_numbers: Vec<u32> = kept.iter().map(|capability| capability.number()).collect();
    for number in (0..=LAST_CAPABILITY).filter(|number| !kept_numbers.contains(number)) {
        // SAFETY: plain values. A number the kernel does not know fails
        // with `EINVAL`, and the ones after it do too.
        let dropped = unsafe { syscall(PRCTL, PR_CAPBSET_DROP, c_long::from(number)) };
        if dropped != 0 {
            break;
        }
    }
    let set = kept.iter().fold(CapabilitySet::empty(), |set, capability| {
        set | CapabilitySet::from_bits_retain(1 << capability.number())
    });
    rustix::thread::set_capabilities(
        None,
        CapabilitySets {
            effective: set,
            permitted: set,
            inheritable: CapabilitySet::empty(),
        },
    )?;
    Ok(())
}

/// The result of a glibc call that returns -1 on failure.
fn checked(result: c_int) -> io::Result<()> {
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Becomes `user`, every thread: groups first, while the process may still
/// change them. Leaving user 0 drops every capability.
fn become_user(user: &User) -> io::Result<()> {
    // SAFETY: `groups` points to `len` group IDs, which glibc only reads.
    checked(unsafe { setgroups(user.groups.len(), user.groups.as_ptr()) })?;
    // SAFETY: plain values; glibc applies the change to every thread.
    checked(unsafe { setresgid(user.gid, user.gid, user.gid) })?;
    // SAFETY: as above.
    checked(unsafe { setresuid(user.uid, user.uid, user.uid) })
}

/// A system call's result: the value, or the error it set.
fn result(value: c_long) -> io::Result<c_long> {
    if value < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(value)
    }
}

/// `landlock_create_ruleset`, `landlock_add_rule` and
/// `landlock_restrict_self`: the same numbers on every architecture.
const LANDLOCK_CREATE_RULESET: c_long = 444;
const LANDLOCK_ADD_RULE: c_long = 445;
const LANDLOCK_RESTRICT_SELF: c_long = 446;

/// `LANDLOCK_CREATE_RULESET_VERSION`: asks the ABI version.
const RULESET_VERSION: c_uint = 1;

/// `LANDLOCK_RULE_PATH_BENEATH`.
const RULE_PATH_BENEATH: c_int = 1;

/// The file access rights of landlock, by bit.
mod fs {
    pub const EXECUTE: u64 = 1 << 0;
    pub const WRITE_FILE: u64 = 1 << 1;
    pub const READ_FILE: u64 = 1 << 2;
    pub const READ_DIR: u64 = 1 << 3;
    pub const REMOVE_DIR: u64 = 1 << 4;
    pub const REMOVE_FILE: u64 = 1 << 5;
    pub const MAKE_DIR: u64 = 1 << 7;
    pub const MAKE_REG: u64 = 1 << 8;
    pub const MAKE_SOCK: u64 = 1 << 9;
    /// Every right of ABI 1.
    pub const ABI_1: u64 = (1 << 13) - 1;
    /// `REFER` (ABI 2).
    pub const REFER: u64 = 1 << 13;
    /// `TRUNCATE` (ABI 3).
    pub const TRUNCATE: u64 = 1 << 14;
    /// `IOCTL_DEV` (ABI 5).
    pub const IOCTL_DEV: u64 = 1 << 15;
    /// The rights that apply to a file rather than a directory.
    pub const FILE: u64 = EXECUTE | WRITE_FILE | READ_FILE | TRUNCATE | IOCTL_DEV;
}

/// The rights a ruleset of ABI `abi` handles: all it knows.
fn handled(abi: u32) -> u64 {
    let mut rights = fs::ABI_1;
    if abi >= 2 {
        rights |= fs::REFER;
    }
    if abi >= 3 {
        rights |= fs::TRUNCATE;
    }
    if abi >= 5 {
        rights |= fs::IOCTL_DEV;
    }
    rights
}

/// The rights `access` grants, of those `abi` handles.
fn rights(access: Access, abi: u32) -> u64 {
    let read = fs::READ_FILE | fs::READ_DIR;
    let write = read | fs::WRITE_FILE | fs::TRUNCATE;
    let granted = match access {
        Access::Read => read,
        Access::ReadWrite => write,
        Access::Device => write | fs::IOCTL_DEV,
        Access::Full => {
            write
                | fs::REMOVE_DIR
                | fs::REMOVE_FILE
                | fs::MAKE_DIR
                | fs::MAKE_REG
                | fs::MAKE_SOCK
                | fs::REFER
        }
    };
    granted & handled(abi) & !fs::EXECUTE
}

/// `struct landlock_ruleset_attr` of ABI 1: the file rights handled.
#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
}

/// `struct landlock_path_beneath_attr`, packed as the kernel declares it.
#[repr(C, packed)]
struct PathBeneath {
    allowed_access: u64,
    parent_fd: i32,
}

/// The landlock ABI version of the kernel; `None` without landlock.
pub fn landlock_abi() -> Option<u32> {
    // SAFETY: with the version flag the kernel reads no attribute.
    let version = unsafe {
        syscall(
            LANDLOCK_CREATE_RULESET,
            std::ptr::null::<RulesetAttr>(),
            0_usize,
            RULESET_VERSION,
        )
    };
    u32::try_from(version).ok().filter(|version| *version > 0)
}

/// Restricts this process's file access to `files`; the ABI used, `None`
/// when the kernel has no landlock. A path that is not there is left out.
fn restrict_files(files: &[(PathBuf, Access)]) -> io::Result<Option<u32>> {
    let Some(abi) = landlock_abi() else {
        return Ok(None);
    };
    let attr = RulesetAttr {
        handled_access_fs: handled(abi),
    };
    // SAFETY: `attr` is a valid ruleset attribute of the size given.
    let fd = result(unsafe {
        syscall(
            LANDLOCK_CREATE_RULESET,
            &raw const attr,
            size_of::<RulesetAttr>(),
            0_u32,
        )
    })?;
    // SAFETY: the kernel returned a new descriptor, which nothing else owns.
    let ruleset = unsafe { OwnedFd::from_raw_fd(c_int::try_from(fd).map_err(io::Error::other)?) };
    for (path, access) in files {
        let Ok(parent) = open_path(path) else {
            continue;
        };
        // A file, not a directory, takes the rights of files alone.
        let is_directory = rustix::fs::fstat(&parent)
            .is_ok_and(|stat| rustix::fs::FileType::from_raw_mode(stat.st_mode).is_dir());
        let mut allowed = rights(*access, abi);
        if !is_directory {
            allowed &= fs::FILE;
        }
        let rule = PathBeneath {
            allowed_access: allowed,
            parent_fd: parent.as_raw_fd(),
        };
        // SAFETY: `rule` is a valid rule of its type, both descriptors open.
        result(unsafe {
            syscall(
                LANDLOCK_ADD_RULE,
                ruleset.as_raw_fd(),
                RULE_PATH_BENEATH,
                &raw const rule,
                0_u32,
            )
        })?;
    }
    // SAFETY: the ruleset descriptor is open; no flags.
    result(unsafe { syscall(LANDLOCK_RESTRICT_SELF, ruleset.as_raw_fd(), 0_u32) })?;
    Ok(Some(abi))
}

/// `path` opened with `O_PATH`, for a landlock rule.
fn open_path(path: &Path) -> io::Result<OwnedFd> {
    use rustix::fs::{Mode, OFlags};
    let fd = rustix::fs::open(path, OFlags::PATH | OFlags::CLOEXEC, Mode::empty())?;
    Ok(File::from(fd).into())
}

mod calls;
#[cfg(coverage)]
mod coverage;
mod filter;

pub use calls::{call_number, is_known_call};
pub use filter::{Filter, Instruction};

#[cfg(test)]
mod tests;
