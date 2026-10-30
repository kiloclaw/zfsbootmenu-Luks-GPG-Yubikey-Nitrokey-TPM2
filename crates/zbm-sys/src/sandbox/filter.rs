//! The seccomp filter: an allow-list of calls, as classic BPF.

use std::ffi::{c_long, c_uint};
use std::io;

use super::{call_number, result, syscall};

/// The architecture the filter checks, as `AUDIT_ARCH_*`.
pub(super) const ARCH: u32 = if cfg!(target_arch = "aarch64") {
    0xC000_00B7
} else {
    0xC000_003E
};

/// The number of `seccomp` on this architecture.
pub(super) const SECCOMP: c_long = if cfg!(target_arch = "aarch64") {
    277
} else {
    317
};

/// `SECCOMP_SET_MODE_FILTER`, and the flags that apply it to every thread
/// and log what it does not allow.
pub(super) const SET_MODE_FILTER: c_uint = 1;
pub(super) const FILTER_FLAG_TSYNC: c_uint = 1;
pub(super) const FILTER_FLAG_LOG: c_uint = 2;

/// Its returns.
pub(super) const RET_KILL_PROCESS: u32 = 0x8000_0000;
pub(super) const RET_ERRNO: u32 = 0x0005_0000;
pub(super) const RET_ALLOW: u32 = 0x7FFF_0000;

/// `EPERM`, the error of a call not allowed.
pub(super) const EPERM: u32 = 1;

/// The classic BPF instructions the filter uses.
pub(super) const LD_W_ABS: u16 = 0x20;
pub(super) const JEQ_K: u16 = 0x15;
pub(super) const JGE_K: u16 = 0x35;
pub(super) const RET_K: u16 = 0x06;

/// The offsets of `struct seccomp_data`: the call's number, then its
/// architecture.
pub(super) const DATA_NR: u32 = 0;
pub(super) const DATA_ARCH: u32 = 4;
/// The low half of the first argument, on these little-endian machines.
pub(super) const DATA_ARG0: u32 = 16;

/// The first number of x32's calls on x86-64, which the filter refuses.
pub(super) const X32_BIT: u32 = 0x4000_0000;

/// `struct sock_filter`: one instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Instruction {
    pub(super) code: u16,
    pub(super) jt: u8,
    pub(super) jf: u8,
    pub(super) k: u32,
}

impl Instruction {
    const fn new(code: u16, jt: u8, jf: u8, k: u32) -> Instruction {
        Instruction { code, jt, jf, k }
    }
}

/// `struct sock_fprog`.
#[repr(C)]
struct Program {
    len: u16,
    filter: *const Instruction,
}

/// A seccomp filter: the calls of a list allowed, the others failing with
/// `EPERM`, another architecture's killing the process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Filter {
    instructions: Vec<Instruction>,
    /// Whether a check is too far from the allow for a jump, which
    /// [`Filter::install`] refuses.
    far: bool,
}

/// An instruction of a filter being built: the jump to the allow is known
/// once every instruction is.
enum Step {
    Plain(Instruction),
    /// Allow when the value loaded is this.
    AllowIf(u32),
}

impl Filter {
    /// A filter that allows the calls named, those this architecture has;
    /// `name/number` only with that first argument.
    pub fn allowing(names: &[&str]) -> Filter {
        let (any, with) = numbers(names);
        let mut code = vec![
            Step::Plain(Instruction::new(LD_W_ABS, 0, 0, DATA_ARCH)),
            Step::Plain(Instruction::new(JEQ_K, 1, 0, ARCH)),
            Step::Plain(Instruction::new(RET_K, 0, 0, RET_KILL_PROCESS)),
            Step::Plain(Instruction::new(LD_W_ABS, 0, 0, DATA_NR)),
        ];
        if cfg!(target_arch = "x86_64") {
            code.push(Step::Plain(Instruction::new(JGE_K, 0, 1, X32_BIT)));
            code.push(Step::Plain(Instruction::new(RET_K, 0, 0, RET_KILL_PROCESS)));
        }
        code.extend(any.into_iter().map(Step::AllowIf));
        for (number, argument) in with {
            // The call, then its first argument, then the number again for
            // the checks after.
            code.push(Step::Plain(Instruction::new(JEQ_K, 0, 3, number)));
            code.push(Step::Plain(Instruction::new(LD_W_ABS, 0, 0, DATA_ARG0)));
            code.push(Step::AllowIf(argument));
            code.push(Step::Plain(Instruction::new(LD_W_ABS, 0, 0, DATA_NR)));
        }
        code.push(Step::Plain(Instruction::new(
            RET_K,
            0,
            0,
            RET_ERRNO | EPERM,
        )));
        code.push(Step::Plain(Instruction::new(RET_K, 0, 0, RET_ALLOW)));
        assemble(code)
    }

    /// Its instructions.
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    /// Installs it on every thread of this process, which must have
    /// `no_new_privs` set.
    ///
    /// # Errors
    ///
    /// When the kernel refuses it: too long, or no `no_new_privs`.
    pub fn install(&self) -> io::Result<()> {
        if self.far {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let program = Program {
            len: u16::try_from(self.instructions.len()).map_err(io::Error::other)?,
            filter: self.instructions.as_ptr(),
        };
        // SAFETY: `program` points to `len` instructions, which live until
        // the call returns; the kernel copies them.
        let unsynced = result(unsafe {
            syscall(
                SECCOMP,
                SET_MODE_FILTER,
                FILTER_FLAG_TSYNC | FILTER_FLAG_LOG,
                &raw const program,
            )
        })?;
        // With `TSYNC`, a thread that could not take the filter is named.
        if unsynced == 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "thread {unsynced} kept its own filter"
            )))
        }
    }
}

/// The numbers of the calls `names` names on this architecture: those
/// allowed whatever their arguments, and those with a first argument.
fn numbers(names: &[&str]) -> (Vec<u32>, Vec<(u32, u32)>) {
    let mut any: Vec<u32> = Vec::new();
    let mut with: Vec<(u32, u32)> = Vec::new();
    for name in names {
        match name.split_once('/') {
            Some((call, argument)) => {
                if let (Some(number), Ok(argument)) = (call_number(call), argument.parse()) {
                    with.push((number, argument));
                }
            }
            None => any.extend(call_number(name)),
        }
    }
    any.sort_unstable();
    any.dedup();
    with.sort_unstable();
    with.dedup();
    (any, with)
}

/// The filter of `code`, whose last step allows: each check jumps there.
fn assemble(code: Vec<Step>) -> Filter {
    let allow = code.len().saturating_sub(1);
    let mut far = false;
    let instructions = code
        .into_iter()
        .enumerate()
        .map(|(at, step)| match step {
            Step::Plain(instruction) => instruction,
            Step::AllowIf(value) => {
                let jump = u8::try_from(allow - at - 1).unwrap_or_else(|_| {
                    far = true;
                    0
                });
                Instruction::new(JEQ_K, jump, 0, value)
            }
        })
        .collect();
    Filter { instructions, far }
}
