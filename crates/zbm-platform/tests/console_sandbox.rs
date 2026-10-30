//! The console's profile applied for real (ROADMAP.md, SE11), in a child
//! process of this program, as `zbm console` applies it before its
//! threads: what the console does still works, the rest does not. No
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

use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;

use zbm_platform::sandbox::{Console, Profile as _, describe};
use zbm_sys::socket::{Connection, Listener};

/// The line of `/proc/self/status` that starts with `key`, without it.
fn status(key: &str) -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix(key).map(|value| value.trim().to_owned()))
        .unwrap()
}

/// The child: confined as the console, in the directories of `base`.
fn confined(base: &str) {
    let base = PathBuf::from(base);
    let (state, sockets) = (base.join("state"), base.join("run"));
    std::fs::create_dir_all(&state).unwrap();
    zbm_sys::socket::make_directory(&sockets.join("console.sock")).unwrap();
    let console = Console {
        state: state.clone(),
        sockets: sockets.clone(),
        share: base.join("share"),
        mirror: None,
    };
    let applied = console.confine().unwrap();
    println!("{}", describe(&applied));
    assert_eq!(status("NoNewPrivs:"), "1");
    assert_eq!(status("Seccomp:"), "2");
    if status("Uid:").starts_with('0') {
        assert_eq!(
            status("CapEff:"),
            format!("{:016x}", (1_u64 << 14) | (1 << 26))
        );
    }
    // A thread, as the control socket's; the socket itself.
    let listener = Listener::bind(&sockets.join("console.sock")).unwrap();
    let path = sockets.join("console.sock");
    let peer = std::thread::spawn(move || {
        let connection = Connection::connect(&path).unwrap();
        connection.send(b"status").unwrap();
    });
    let accepted = listener
        .accept(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let message = accepted
        .unwrap()
        .receive(Some(std::time::Duration::from_secs(5)));
    assert_eq!(message.unwrap().unwrap(), b"status");
    peer.join().unwrap();
    // State files, written, renamed and removed.
    std::fs::write(state.join("console.tmp"), b"1\n").unwrap();
    std::fs::rename(state.join("console.tmp"), state.join("console")).unwrap();
    std::fs::remove_file(state.join("console")).unwrap();
    // The kernel's view, as the console reads it.
    assert!(std::fs::read_dir("/sys/class").is_ok());
    // No program runs.
    let refused = Command::new("/bin/true").status().unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::PermissionDenied, "{refused}");
    // Nothing outside its places, where the kernel has landlock.
    if applied.landlock.is_some() {
        let other = std::fs::read_dir("/etc").unwrap_err();
        assert_eq!(other.kind(), ErrorKind::PermissionDenied);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("confined") {
        confined(&args.next().unwrap());
        return;
    }
    let base = std::env::temp_dir().join(format!("zbm-console-sandbox-{}", std::process::id()));
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("confined")
        .arg(&base)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&base);
    let said = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{} {said}{stderr}", output.status);
    println!("the console's profile: {}", said.trim());
}
