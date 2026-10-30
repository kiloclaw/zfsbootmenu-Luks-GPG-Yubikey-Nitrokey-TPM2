//! The profile of the menu over SSH applied for real (ROADMAP.md, SE11), in
//! a child process of this program, as `zfsbootmenu` applies it once it
//! attached to the session: what the menu does still works, the rest does
//! not. No harness (`harness = false`): its threads would be confined too.

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

use std::io::{ErrorKind, Write as _};
use std::os::fd::AsFd as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use zbm_platform::sandbox::{Menu, Profile as _, describe};
use zbm_sys::socket::{Connection, Listener};

/// The line of `/proc/self/status` that starts with `key`, without it, read
/// before the process is confined.
fn status(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|line| line.strip_prefix(key).map(|value| value.trim().to_owned()))
        .unwrap()
}

/// The child: confined as the menu, in the directories of `base`.
fn confined(base: &str) {
    let base = PathBuf::from(base);
    let (state, sockets) = (base.join("state"), base.join("run"));
    std::fs::create_dir_all(&state).unwrap();
    let path = sockets.join("console.sock");
    zbm_sys::socket::make_directory(&path).unwrap();
    // What the menu has before it is confined: the console listening, and
    // its connection to the session.
    let console = Listener::bind(&path).unwrap();
    let (menu, session) = Connection::pair().unwrap();
    // `/proc` is out of reach once confined: its own status is kept open.
    let mut proc_status = std::fs::File::open("/proc/self/status").unwrap();
    let menu_profile = Menu {
        state: state.clone(),
        sockets: sockets.clone(),
    };

    let applied = menu_profile.confine().unwrap();

    println!("{}", describe(&applied));
    let text = {
        use std::io::{Read as _, Seek as _};
        let mut text = String::new();
        proc_status.rewind().unwrap();
        proc_status.read_to_string(&mut text).unwrap();
        text
    };
    assert_eq!(status(&text, "NoNewPrivs:"), "1");
    assert_eq!(status(&text, "Seccomp:"), "2");
    assert_eq!(status(&text, "CapEff:"), "0000000000000000");
    // The session's connection, a descriptor with a message.
    menu.send_with(b"attach", &[menu.as_fd()]).unwrap();
    let (message, fds) = session
        .receive_with(Some(Duration::from_secs(5)))
        .unwrap()
        .unwrap();
    assert_eq!((message.as_slice(), fds.len()), (&b"attach"[..], 1));
    // The console's socket, which it tells of a secret prompt.
    let told = Connection::connect(&path).unwrap();
    told.send(b"secret").unwrap();
    drop(console);
    // Its log, appended to, and a crash report written and renamed.
    let mut journal = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("journal.jsonl"))
        .unwrap();
    journal.write_all(b"{}\n").unwrap();
    std::fs::write(state.join("crash.tmp"), b"report\n").unwrap();
    std::fs::rename(state.join("crash.tmp"), state.join("crash")).unwrap();
    std::fs::remove_file(state.join("crash")).unwrap();
    // No program runs, and no thread starts.
    let refused = Command::new("/bin/true").status().unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::PermissionDenied, "{refused}");
    assert!(std::thread::Builder::new().spawn(|| ()).is_err());
    // Nothing outside its places, where the kernel has landlock.
    if applied.landlock.is_some() {
        for other in ["/etc", "/sys", "/proc/self"] {
            let error = std::fs::read_dir(other).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{other}");
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("confined") {
        confined(&args.next().unwrap());
        return;
    }
    let base = std::env::temp_dir().join(format!("zbm-menu-sandbox-{}", std::process::id()));
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("confined")
        .arg(&base)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&base);
    let said = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{} {said}{stderr}", output.status);
    println!("the menu's profile: {}", said.trim());
}
