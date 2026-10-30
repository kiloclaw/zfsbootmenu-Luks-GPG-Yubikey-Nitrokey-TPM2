use super::calls::CALLS;
use super::filter::*;
use super::*;

/// A call of the table: its name and numbers.
type Call = (&'static str, Option<u32>, Option<u32>);

/// Where a filter goes for the call numbered `nr` on this architecture, as
/// the kernel would run it: the value it returns.
fn run(filter: &Filter, arch: u32, nr: u32) -> u32 {
    run_with(filter, arch, nr, 0)
}

/// As [`run`], with the call's first argument.
fn run_with(filter: &Filter, arch: u32, nr: u32, arg0: u32) -> u32 {
    let mut accumulator = 0;
    let mut at = 0;
    while let Some(instruction) = filter.instructions().get(at) {
        at += 1;
        match instruction.code {
            LD_W_ABS => {
                accumulator = match instruction.k {
                    DATA_ARCH => arch,
                    DATA_ARG0 => arg0,
                    _ => nr,
                }
            }
            JEQ_K | JGE_K => {
                let taken = if instruction.code == JEQ_K {
                    accumulator == instruction.k
                } else {
                    accumulator >= instruction.k
                };
                at += usize::from(if taken {
                    instruction.jt
                } else {
                    instruction.jf
                });
            }
            RET_K => return instruction.k,
            other => panic!("instruction {other:#x}"),
        }
    }
    panic!("the filter fell through");
}

#[test]
fn the_filter_allows_its_calls_alone_and_this_architecture_alone() {
    let filter = Filter::allowing(&["read", "write", "read", "no_such_call", "exit_group"]);
    let number = |name| call_number(name).unwrap();

    for name in ["read", "write", "exit_group"] {
        assert_eq!(run(&filter, ARCH, number(name)), RET_ALLOW, "{name}");
    }
    for name in ["openat", "socket", "execve"] {
        assert_eq!(
            run(&filter, ARCH, number(name)),
            RET_ERRNO | EPERM,
            "{name}"
        );
    }
    assert_eq!(run(&filter, 0x4000_0003, number("read")), RET_KILL_PROCESS);
    if cfg!(target_arch = "x86_64") {
        assert_eq!(
            run(&filter, ARCH, X32_BIT | number("read")),
            RET_KILL_PROCESS
        );
    }
    // The three calls once each, and none of the unknown.
    let checks = filter
        .instructions()
        .iter()
        .filter(|instruction| instruction.code == JEQ_K && instruction.k != ARCH)
        .count();
    assert_eq!(checks, 3);
}

#[test]
fn a_long_list_still_reaches_the_allow() {
    let names: Vec<&str> = CALLS.iter().map(|(name, _, _)| *name).collect();
    let filter = Filter::allowing(&names);

    for name in &names {
        if let Some(number) = call_number(name) {
            assert_eq!(run(&filter, ARCH, number), RET_ALLOW, "{name}");
        }
    }
    assert_eq!(run(&filter, ARCH, 1023), RET_ERRNO | EPERM);
}

#[test]
fn the_table_names_each_call_once_with_the_numbers_of_both_architectures() {
    let mut names: Vec<&str> = CALLS.iter().map(|(name, _, _)| *name).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(names.len(), before);
    // Spot checks of both tables.
    assert_eq!(
        call_number("openat"),
        Some(if cfg!(target_arch = "aarch64") {
            56
        } else {
            257
        })
    );
    assert_eq!(call_number("memfd_secret"), Some(447));
    let numbers = |pick: fn(&Call) -> Option<u32>| {
        let mut numbers: Vec<u32> = CALLS.iter().filter_map(pick).collect();
        numbers.sort_unstable();
        let before = numbers.len();
        numbers.dedup();
        before == numbers.len()
    };
    assert!(numbers(|call| call.1), "x86-64 numbers repeat");
    assert!(numbers(|call| call.2), "arm64 numbers repeat");
}

#[test]
fn landlock_grants_what_each_access_needs_of_what_the_kernel_knows() {
    assert_eq!(handled(1), fs::ABI_1);
    assert_eq!(
        handled(5),
        fs::ABI_1 | fs::REFER | fs::TRUNCATE | fs::IOCTL_DEV
    );
    let read = fs::READ_FILE | fs::READ_DIR;
    assert_eq!(rights(Access::Read, 5), read);
    assert_eq!(rights(Access::ReadWrite, 1), read | fs::WRITE_FILE);
    assert_eq!(
        rights(Access::ReadWrite, 3),
        read | fs::WRITE_FILE | fs::TRUNCATE
    );
    assert_eq!(rights(Access::Device, 4) & fs::IOCTL_DEV, 0);
    assert_ne!(rights(Access::Device, 5) & fs::IOCTL_DEV, 0);
    assert_eq!(rights(Access::Full, 1) & fs::REFER, 0);
    assert_ne!(rights(Access::Full, 2) & fs::MAKE_REG, 0);
    for access in [
        Access::Read,
        Access::ReadWrite,
        Access::Device,
        Access::Full,
    ] {
        assert_eq!(rights(access, 6) & fs::EXECUTE, 0, "{access:?}");
    }
}

#[test]
fn a_call_can_be_allowed_for_one_first_argument() {
    let filter = Filter::allowing(&["prctl/15", "write", "prctl/no", "nothing/3"]);
    let prctl = call_number("prctl").unwrap();

    assert_eq!(run_with(&filter, ARCH, prctl, 15), RET_ALLOW);
    assert_eq!(run_with(&filter, ARCH, prctl, 4), RET_ERRNO | EPERM);
    assert_eq!(
        run_with(&filter, ARCH, call_number("write").unwrap(), 4),
        RET_ALLOW
    );
    assert_eq!(
        run_with(&filter, ARCH, call_number("read").unwrap(), 15),
        RET_ERRNO | EPERM
    );
    assert_eq!(Capability::IpcLock.number(), 14);
    assert_eq!(Capability::SysTtyConfig.number(), 26);
}
