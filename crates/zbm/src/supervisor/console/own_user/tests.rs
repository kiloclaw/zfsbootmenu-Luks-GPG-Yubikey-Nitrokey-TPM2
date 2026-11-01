#![allow(
    clippy::disallowed_methods,
    reason = "the records directory and the socket are real, in a temporary directory"
)]
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests may panic; the rules are for process 1"
)]

use std::os::unix::fs::MetadataExt as _;

use pretty_assertions::assert_eq;
use zbm_display::devices::{Kind, Opener as _};
use zbm_testkit::TempDir;

use super::*;

/// The places of a console's own user in `dir`, the user being this
/// process's own, so that it may give the directory away.
fn own_user(dir: &TempDir) -> OwnUser {
    OwnUser {
        uid: zbm_sys::credentials().uid,
        records: dir.join("zfsbootmenu/console"),
        socket: dir.join("run/console.sock"),
        kmsg: dir.write("kmsg", ""),
        dev: dir.join("dev"),
    }
}

#[test]
fn the_system_s_places_are_the_image_s() {
    let own = OwnUser::system();
    assert_eq!(own.uid, CONSOLE_USER);
    assert_eq!(own.records, Path::new("/zfsbootmenu/console"));
    assert_eq!(own.socket, Path::new("/run/zfsbootmenu/console.sock"));
    assert_eq!(own.kmsg, Path::new("/dev/kmsg"));
}

#[test]
fn the_console_gets_its_directory_socket_log_and_broker() {
    let dir = TempDir::new("zbm-own-user");
    let own = own_user(&dir);
    let mut command = Command::new("/bin/true");

    let broker = own.prepare(&mut command).unwrap();

    let records = std::fs::metadata(&own.records).unwrap();
    assert!(records.is_dir());
    assert_eq!(records.uid(), own.uid);
    assert_eq!(records.mode() & 0o777, 0o755);
    // Bound, and left there for the console that serves it.
    assert!(own.socket.exists());
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let flags: Vec<&str> = args.iter().step_by(2).map(String::as_str).collect();
    assert_eq!(flags, ["--control-fd", "--devices-fd", "--kmsg-fd"]);
    assert!(
        args.iter()
            .skip(1)
            .step_by(2)
            .all(|fd| fd.parse::<i32>().is_ok())
    );

    // The broker refuses what is not a device of the kind asked for.
    let (console, supervisor) = Connection::pair().unwrap();
    drop(broker);
    serve_devices(supervisor, own.dev.clone(), |_| {}).unwrap();
    let asked = zbm_app::devices::Broker::new(console);
    let error = asked
        .open(Kind::Card, &dir.join("dev/dri/card0"))
        .unwrap_err();
    assert!(error.to_string().contains("card0"), "{error}");
}

#[test]
fn what_cannot_be_had_is_an_error() {
    let dir = TempDir::new("zbm-own-user-missing");
    let own = OwnUser {
        kmsg: dir.join("no/kmsg"),
        ..own_user(&dir)
    };
    let mut command = Command::new("/bin/true");
    assert!(own.prepare(&mut command).is_err());
}

/// A program that writes its user, group and effective capabilities to
/// `out`, then waits for its terminal to close.
fn reporter(out: &Path) -> crate::supervisor::Program {
    crate::supervisor::Program {
        path: PathBuf::from("/bin/sh"),
        args: vec![
            "-c".to_owned(),
            format!(
                "{{ id -u; id -g; grep CapEff /proc/self/status; }} > {}",
                out.display()
            ),
        ],
    }
}

/// Waits up to five seconds for `path` to hold three lines.
fn report(path: &Path) -> Vec<String> {
    for _ in 0..50 {
        if let Ok(text) = std::fs::read_to_string(path)
            && text.lines().count() >= 3
        {
            return text.lines().map(str::to_owned).collect();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Vec::new()
}

#[test]
fn the_console_runs_as_its_own_user_with_one_capability() {
    if zbm_sys::credentials().uid != 0 {
        return;
    }
    let dir = TempDir::new("zbm-own-user-start");
    let own = OwnUser {
        uid: CONSOLE_USER,
        ..own_user(&dir)
    };
    let out = own.records.join("id");
    let mut notes = Vec::new();

    let attached =
        super::super::Attached::start_as(&reporter(&out), Some(&own), &mut |note: &str| {
            notes.push(note.to_owned());
        })
        .unwrap();

    let lines = report(&out);
    drop(attached);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(
        lines.first().map(String::as_str),
        Some("60600"),
        "{lines:?}"
    );
    assert_eq!(lines.get(1).map(String::as_str), Some("60600"), "{lines:?}");
    let kept = format!("{:016x}", 1_u64 << 26);
    assert!(
        lines.get(2).is_some_and(|line| line.ends_with(&kept)),
        "{lines:?}"
    );
}

#[test]
fn a_console_that_cannot_have_its_user_runs_as_root_and_says_why() {
    let dir = TempDir::new("zbm-own-user-fallback");
    let own = OwnUser {
        kmsg: dir.join("no/kmsg"),
        ..own_user(&dir)
    };
    let out = dir.join("id");
    let mut notes = Vec::new();

    let attached =
        super::super::Attached::start_as(&reporter(&out), Some(&own), &mut |note: &str| {
            notes.push(note.to_owned());
        })
        .unwrap();

    let lines = report(&out);
    drop(attached);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("runs as root"), "{notes:?}");
    let uid = zbm_sys::credentials().uid.to_string();
    assert_eq!(lines.first(), Some(&uid), "{lines:?}");
}

#[test]
fn the_console_s_crash_reports_are_taken_as_files_alone() {
    let dir = TempDir::new("zbm-own-user-crashes");
    let records = dir.join("console");
    std::fs::create_dir_all(&records).unwrap();
    std::fs::write(records.join("crash-1700000000-77.txt"), "panicked").unwrap();
    std::fs::write(records.join("pid"), "77\n").unwrap();
    let secret = dir.write("secret", "root's");
    std::os::unix::fs::symlink(&secret, records.join("crash-1700000001-77.txt")).unwrap();
    let big = vec![b'x'; usize::try_from(MAX_REPORT).unwrap() + 1];
    std::fs::write(records.join("crash-1700000002-77.txt"), big).unwrap();

    let adopted = adopt_crash_reports(&records, dir.path());

    assert_eq!(adopted, 1);
    assert_eq!(
        std::fs::read_to_string(dir.join("crash-1700000000-77.txt")).unwrap(),
        "panicked"
    );
    assert!(!dir.join("crash-1700000001-77.txt").exists());
    assert!(!dir.join("crash-1700000002-77.txt").exists());
    assert!(records.join("pid").exists());
    assert!(!records.join("crash-1700000000-77.txt").exists());
    assert_eq!(adopt_crash_reports(&dir.join("none"), dir.path()), 0);
}
