//! `zfsbootmenu` over SSH confines itself once it attached to the session
//! (ROADMAP.md, SE11), here in child processes of this program: with its
//! places, and without its state directory, which it then cannot make. No
//! harness (`harness = false`): its threads would be confined too.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::disallowed_macros,
    reason = "a failing check is meant to panic"
)]
#![allow(
    clippy::disallowed_methods,
    reason = "the check starts this program again and works with real files"
)]

use std::io::{ErrorKind, Read as _, Seek as _};
use std::path::PathBuf;
use std::process::Command;

/// The value of `key` in the status the process reads from `file`.
fn status(file: &mut std::fs::File, key: &str) -> String {
    let mut text = String::new();
    file.rewind().unwrap();
    file.read_to_string(&mut text).unwrap();
    text.lines()
        .find_map(|line| line.strip_prefix(key).map(|value| value.trim().to_owned()))
        .unwrap()
}

/// The child: `zfsbootmenu`'s confinement, in the directories of `base`,
/// the state's made unless `missing`.
fn confined(base: &str, missing: bool) {
    let base = PathBuf::from(base);
    let (state, sockets) = (base.join("state"), base.join("run"));
    std::fs::create_dir_all(&sockets).unwrap();
    if !missing {
        std::fs::create_dir_all(&state).unwrap();
    }
    let mut own = std::fs::File::open("/proc/self/status").unwrap();

    zbm::menu::confine_remote(&state, &sockets);

    assert_eq!(status(&mut own, "NoNewPrivs:"), "1");
    assert_eq!(status(&mut own, "Seccomp:"), "2");
    assert_eq!(status(&mut own, "CapEff:"), "0000000000000000");
    let refused = Command::new("/bin/true").status().unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::PermissionDenied, "{refused}");
    if missing {
        // A place that was not there is out of reach, where the kernel has
        // landlock.
        if zbm_sys::sandbox::landlock_abi().is_some() {
            let error = std::fs::create_dir_all(&state).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{error}");
        }
    } else {
        std::fs::write(state.join("journal.jsonl"), b"{}\n").unwrap();
    }
    // Confined again, it is not: its filter refuses the calls that would
    // confine it, which is logged, and it runs on.
    zbm::menu::confine_remote(&state, &sockets);
    assert_eq!(status(&mut own, "Seccomp:"), "2");
}

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("confined") {
        let base = args.next().unwrap();
        confined(&base, args.next().as_deref() == Some("missing"));
        return;
    }
    for case in ["placed", "missing"] {
        let base =
            std::env::temp_dir().join(format!("zbm-menu-confine-{}-{case}", std::process::id()));
        let output = Command::new(std::env::current_exe().unwrap())
            .arg("confined")
            .arg(&base)
            .arg(case)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&base);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "{case}: {} {stderr}",
            output.status
        );
        println!("zfsbootmenu confines itself: {case}");
    }
}
