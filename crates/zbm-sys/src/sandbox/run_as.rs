//! A child process that runs its program as another user (ROADMAP.md
//! SE11, ADR 0012): the supervisor starts the console on the display so,
//! as its own user with one capability kept, and its locked memory
//! unbounded. All of it happens between `fork` and `exec`, in the child
//! alone, which has one thread.

use std::io;

use rustix::process::{Gid, Resource, Rlimit, Uid, setrlimit};
use rustix::thread::{
    CapabilitySet, CapabilitySets, configure_capability_in_ambient_set, set_capabilities,
    set_keep_capabilities, set_thread_groups, set_thread_res_gid, set_thread_res_uid,
};

use super::Capability;

/// The user a child process becomes before it runs its program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunAs {
    /// Its user and group, with no supplementary group.
    pub uid: u32,
    /// Its group.
    pub gid: u32,
    /// The capabilities it keeps across the change and its `exec`, as
    /// ambient capabilities: they are its effective ones then.
    pub kept: Vec<Capability>,
    /// Whether `RLIMIT_MEMLOCK` is lifted first, where it may be, so that
    /// it can lock all its memory without `CAP_IPC_LOCK`.
    pub memlock_unlimited: bool,
}

/// What a user change needs, made before the fork: nothing is allocated
/// after it.
#[derive(Clone, Debug)]
struct Change {
    uid: Uid,
    gid: Gid,
    /// Each capability kept, as a set of one.
    kept: Vec<CapabilitySet>,
    /// All of them.
    set: CapabilitySet,
    memlock_unlimited: bool,
}

impl Change {
    /// The change `user` asks for.
    fn of(user: &RunAs) -> Change {
        let kept: Vec<CapabilitySet> = user
            .kept
            .iter()
            .map(|capability| CapabilitySet::from_bits_retain(1 << capability.number()))
            .collect();
        let set = kept
            .iter()
            .fold(CapabilitySet::empty(), |set, one| set | *one);
        Change {
            uid: Uid::from_raw(user.uid),
            gid: Gid::from_raw(user.gid),
            kept,
            set,
            memlock_unlimited: user.memlock_unlimited,
        }
    }

    /// Applies it to the calling thread, and allocates nothing.
    fn apply(&self) -> io::Result<()> {
        if self.memlock_unlimited {
            let unlimited = Rlimit {
                current: None,
                maximum: None,
            };
            // Without `CAP_SYS_RESOURCE`, as in some containers, the limit
            // stays: the program says so when it cannot lock its memory,
            // and runs on.
            let _ = setrlimit(Resource::Memlock, unlimited);
        }
        // The permitted capabilities stay across the user change; the
        // effective ones go, and are set again.
        set_keep_capabilities(true)?;
        set_thread_groups(&[])?;
        set_thread_res_gid(self.gid, self.gid, self.gid)?;
        set_thread_res_uid(self.uid, self.uid, self.uid)?;
        let sets = CapabilitySets {
            effective: self.set,
            permitted: self.set,
            inheritable: self.set,
        };
        set_capabilities(None, sets)?;
        for capability in &self.kept {
            configure_capability_in_ambient_set(*capability, true)?;
        }
        set_keep_capabilities(false)?;
        Ok(())
    }
}

/// Changes the calling thread as [`run_as`] changes a child, for a process
/// of one thread: the tests of the change itself.
///
/// # Errors
///
/// When a step fails; the steps before it stay done.
pub fn change_thread(user: &RunAs) -> io::Result<()> {
    Change::of(user).apply()
}

/// Has the program that `process` starts run as `user` says.
pub fn run_as(process: &mut std::process::Command, user: &RunAs) {
    use std::os::unix::process::CommandExt;
    let change = Change::of(user);
    // SAFETY: the closure runs between `fork` and `exec`, in a child of
    // one thread, where only async-signal-safe functions may be called: it
    // makes the `prlimit64`, `prctl`, `setgroups`, `setresgid`,
    // `setresuid` and `capset` system calls on values made before the
    // fork, and allocates nothing.
    unsafe {
        process.pre_exec(move || change.apply());
    }
}
