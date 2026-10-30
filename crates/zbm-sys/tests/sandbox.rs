//! Confinement applied for real (ROADMAP.md, SE11), each case in a child
//! process of this program, which it cannot undo: the parent starts this
//! program again with the case's name, and the child confines itself and
//! checks what it can still do. No harness (`harness = false` in
//! `Cargo.toml`): its threads would be confined with the case.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::disallowed_macros,
    reason = "a failing check is meant to panic"
)]
#![allow(
    clippy::disallowed_methods,
    reason = "the checks start this program again and read /proc"
)]

use std::io::ErrorKind;
use std::process::Command;

use zbm_sys::sandbox::{Access, Capability, Confinement, User, confine, landlock_abi, prepare};

/// What any Rust program needs to print, end a thread and exit.
const BASE: &[&str] = &[
    "read",
    "write",
    "close",
    "fstat",
    "statx",
    "openat",
    "lseek",
    "mmap",
    "munmap",
    "mprotect",
    "madvise",
    "brk",
    "futex",
    "rt_sigprocmask",
    "rt_sigaction",
    "sigaltstack",
    "sched_yield",
    "exit",
    "exit_group",
];

/// The line of `/proc/self/status` that starts with `key`, without it.
fn status(key: &str) -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix(key).map(|value| value.trim().to_owned()))
        .unwrap()
}

/// Whether making a socket pair is refused with `EPERM`.
fn sockets_refused() -> bool {
    match std::os::unix::net::UnixStream::pair() {
        Err(error) => error.kind() == ErrorKind::PermissionDenied,
        Ok(_) => false,
    }
}

fn filter() {
    assert_eq!(status("NoNewPrivs:"), "0");
    let applied = confine(&Confinement {
        calls: Some(BASE),
        ..Confinement::default()
    })
    .unwrap();
    assert!(applied.seccomp && !applied.user);
    assert_eq!(status("NoNewPrivs:"), "1");
    assert_eq!(status("Seccomp:"), "2");
    assert!(sockets_refused());
}

fn threads() {
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let (done, result) = std::sync::mpsc::channel();
    // A thread that started before: the filter reaches it too.
    let thread = std::thread::spawn(move || {
        wait.recv().unwrap();
        done.send(sockets_refused()).unwrap();
    });
    let mut calls = BASE.to_vec();
    calls.extend(["clone", "clone3", "set_robust_list", "rseq", "getrandom"]);
    let calls: &'static [&'static str] = calls.leak();
    confine(&Confinement {
        calls: Some(calls),
        ..Confinement::default()
    })
    .unwrap();
    go.send(()).unwrap();
    assert!(result.recv().unwrap(), "the earlier thread is filtered");
    thread.join().unwrap();
}

fn user() {
    if status("Uid:").split_whitespace().next() != Some("0") {
        println!("skipped: not root");
        return;
    }
    let applied = confine(&Confinement {
        user: Some(User {
            uid: 65534,
            gid: 65534,
            groups: vec![65533],
        }),
        ..Confinement::default()
    })
    .unwrap();
    assert!(applied.user);
    assert_eq!(status("Uid:"), "65534\t65534\t65534\t65534");
    assert_eq!(status("Gid:"), "65534\t65534\t65534\t65534");
    assert_eq!(status("Groups:"), "65533");
    assert_eq!(status("CapEff:"), "0000000000000000");
    assert_eq!(status("CapPrm:"), "0000000000000000");
    assert_eq!(status("NoNewPrivs:"), "1");
}

/// Root becomes root again, without its supplementary groups: the same
/// calls as for another user, in a process that can still write what it
/// must (a coverage build's profile).
fn same_user() {
    if status("Uid:").split_whitespace().next() != Some("0") {
        println!("skipped: not root");
        return;
    }
    let applied = confine(&Confinement {
        user: Some(User {
            uid: 0,
            gid: 0,
            groups: Vec::new(),
        }),
        ..Confinement::default()
    })
    .unwrap();
    assert!(applied.user);
    assert_eq!(status("Groups:"), "");
    assert_eq!(status("NoNewPrivs:"), "1");
}

fn files() {
    let applied = confine(&Confinement {
        files: Some(vec![
            ("/proc".into(), Access::Read),
            ("/nowhere".into(), Access::Read),
        ]),
        ..Confinement::default()
    })
    .unwrap();
    let Some(abi) = applied.landlock else {
        println!("skipped: the kernel has no landlock");
        return;
    };
    assert!(abi >= 1);
    assert_eq!(status("NoNewPrivs:"), "1");
    let other = std::fs::read_to_string("/etc/hostname")
        .or_else(|_| std::fs::read("/").map(|_| String::new()));
    assert_eq!(other.unwrap_err().kind(), ErrorKind::PermissionDenied);
}

/// A program started confined: no new privileges, another user when this
/// is root; files cannot be confined before `exec`.
fn children() {
    let root = status("Uid:").split_whitespace().next() == Some("0");
    let user = root.then(|| User {
        uid: 65534,
        gid: 65534,
        groups: Vec::new(),
    });
    let mut command = Command::new("cat");
    command.arg("/proc/self/status");
    prepare(
        &mut command,
        &Confinement {
            user: user.clone(),
            ..Confinement::default()
        },
    )
    .unwrap();
    let output = command.output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("NoNewPrivs:\t1"), "{text}");
    if root {
        assert!(text.contains("Uid:\t65534\t65534\t65534\t65534"), "{text}");
    }
    let files = Confinement {
        files: Some(Vec::new()),
        ..Confinement::default()
    };
    let refused = prepare(&mut Command::new("true"), &files).unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::Unsupported);
}

/// Root keeps the capabilities named, and nothing it runs gets the others
/// back; `prctl` names threads alone.
fn capabilities() {
    if status("Uid:").split_whitespace().next() != Some("0") {
        println!("skipped: not root");
        return;
    }
    let mut calls = BASE.to_vec();
    calls.push("prctl/15");
    let calls: &'static [&'static str] = calls.leak();
    confine(&Confinement {
        capabilities: Some(vec![Capability::IpcLock, Capability::SysTtyConfig]),
        calls: Some(calls),
        files: Some(vec![
            ("/proc".into(), Access::Read),
            ("/dev/null".into(), Access::ReadWrite),
        ]),
        ..Confinement::default()
    })
    .unwrap();
    let kept = format!("{:016x}", (1_u64 << 14) | (1 << 26));
    assert_eq!(status("CapEff:"), kept);
    assert_eq!(status("CapPrm:"), kept);
    assert_eq!(status("CapBnd:"), kept);
    assert_eq!(status("CapInh:"), "0000000000000000");
    // A file alone, with the rights of files.
    std::fs::write("/dev/null", b"x").unwrap();
    assert!(rustix::thread::set_name(c"confined").is_ok());
    let dumpable =
        rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::Dumpable);
    assert_eq!(dumpable.unwrap_err(), rustix::io::Errno::PERM);
}

fn main() {
    children();
    let cases: [(&str, fn()); 6] = [
        ("capabilities", capabilities),
        ("same-user", same_user),
        ("filter", filter),
        ("threads", threads),
        ("user", user),
        ("files", files),
    ];
    // A case's name runs that case, in this child; anything else, such as
    // a test filter, runs them all.
    if let Some(name) = std::env::args().nth(1)
        && let Some((_, case)) = cases.iter().find(|(case, _)| *case == name)
    {
        case();
        return;
    }
    println!("landlock ABI: {:?}", landlock_abi());
    for (name, _) in cases {
        let output = Command::new(std::env::current_exe().unwrap())
            .arg(name)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "{name}: {} {said}{stderr}",
            output.status
        );
        println!("{name}: ok {}", said.trim());
    }
}
