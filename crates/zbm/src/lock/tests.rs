use std::io;
use std::time::Duration;

use pretty_assertions::assert_eq;
use zbm_app::keyholder::{FromKeyholder, Keyholder, ToKeyholder};
use zbm_sys::socket::Connection;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// Keyholders that check each PIN against `pin`, in a thread of their own.
struct Checking {
    pin: &'static [u8],
}

/// A keyholder that answers one PIN, through `connection`.
struct Answering {
    connection: Connection,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Keyholder for Answering {
    fn connection(&self) -> &Connection {
        &self.connection
    }
}

impl Drop for Answering {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Keyholders for Checking {
    fn start(&self) -> io::Result<Box<dyn Keyholder + '_>> {
        let (ours, theirs) = Connection::pair()?;
        let pin = self.pin;
        let thread = zbm_platform::event::spawn_traced("keyholder", move || {
            let request = theirs.receive(None).unwrap().unwrap();
            let Ok(ToKeyholder::Verify { length }) = ToKeyholder::decode(&request) else {
                return;
            };
            let typed = if length == 0 {
                Vec::new()
            } else {
                theirs.receive(None).unwrap().unwrap()
            };
            let verified = FromKeyholder::Verified(typed == pin);
            theirs.send(&verified.encode()).unwrap();
        })?;
        Ok(Box::new(Answering {
            connection: ours,
            thread: Some(thread),
        }))
    }
}

/// The locked image's authority.
fn locked() -> Authority {
    Authority::new(Lock::On("hash".to_owned()))
}

#[test]
fn the_right_pin_opens_the_shell_and_the_lock_closes_after_it() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    let state = State::new(&fake.files, "/zfsbootmenu");
    fake.console.secret(b"wrong pin").secret(b"correct horse");
    let authority = locked();
    let keyholders = Checking {
        pin: b"correct horse",
    };

    let status = ask_then_shell(&fake.system(), &state, (&authority, &keyholders));

    assert_eq!(status, 0);
    let shown = fake.console.output_text();
    assert!(shown.starts_with(LOCKED), "{shown}");
    assert_eq!(shown.matches("PIN: ").count(), 2, "{shown}");
    assert!(shown.contains("Wrong PIN.\n"), "{shown}");
    let ran = fake.runner.lines();
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert!(ran[0].ends_with(" -i"), "{ran:?}");
    assert!(authority.is_locked(false, Duration::ZERO));
}

#[test]
fn a_blank_line_or_three_wrong_pins_go_back_without_a_shell() {
    let keyholders = Checking {
        pin: b"correct horse",
    };
    for typed in [&[""][..], &["a", "b", "c", "correct horse"][..]] {
        let fake = FakeSystem::new();
        fake.files.dir("/zfsbootmenu");
        let state = State::new(&fake.files, "/zfsbootmenu");
        for line in typed {
            fake.console.secret(line.as_bytes());
        }

        let status = ask_then_shell(&fake.system(), &state, (&locked(), &keyholders));

        assert_eq!(status, 1);
        assert_eq!(fake.runner.lines(), Vec::<String>::new());
        assert!(
            fake.console
                .output_text()
                .ends_with("Back to ZFSBootMenu.\n")
        );
    }
}

#[test]
fn a_person_who_opened_a_volume_or_an_open_image_get_the_shell_at_once() {
    let keyholders = Checking { pin: b"" };
    for (lock, attended) in [(Lock::Off, false), (Lock::On("hash".to_owned()), true)] {
        let fake = FakeSystem::new();
        fake.files.dir("/zfsbootmenu");
        let state = State::new(&fake.files, "/zfsbootmenu");
        if attended {
            state
                .record_unlocked("home", zbm_state::Unlocked::Attended)
                .unwrap();
        }

        let status = ask_then_shell(&fake.system(), &state, (&Authority::new(lock), &keyholders));

        assert_eq!(status, 0);
        assert_eq!(fake.console.output_text(), "");
        assert_eq!(fake.runner.lines().len(), 1);
    }
}

#[test]
fn the_usage_is_shown_for_anything_else() {
    let (status, _, stderr) = call(crate::APPLETS, "zbm", &["lock", "open"]);
    assert_eq!(status, USAGE);
    assert!(stderr.contains("zbm lock shell"), "{stderr}");
    let (status, stdout, _) = call(crate::APPLETS, "zbm", &["lock", "-h"]);
    assert_eq!(status, 0);
    assert!(stdout.contains("zbm lock status"), "{stdout}");
}

/// `zbm` run with `args`: its status, standard output and error.
fn call(applets: &[crate::Applet], argv0: &str, args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = crate::Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    let status = crate::run_with(applets, argv0, &args, &mut io);
    (
        status,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    )
}

#[test]
fn a_new_pin_typed_twice_prints_the_line_that_locks_an_image() {
    let fake = FakeSystem::new();
    fake.files.file("/dev/urandom", "0123456789abcdef-and-more");
    let typed = |first: &[u8], second: &[u8]| {
        fake.console.secret(first).secret(second);
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let status = hash(
            &fake.system(),
            &mut crate::Io {
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        );
        (
            status,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    };

    let (status, line, _) = typed(b"correct horse", b"correct horse");
    assert_eq!(status, 0);
    let hash = line
        .strip_prefix("zfsbootmenu_lock_hash='")
        .and_then(|rest| rest.strip_suffix("'\n"))
        .unwrap();
    assert!(
        hash.starts_with("$argon2id$v=19$m=65536,t=3,p=1$"),
        "{hash}"
    );
    assert_eq!(
        typed(b"correct horse", b"correct horsf"),
        (
            1,
            String::new(),
            "zbm lock: the two PINs differ\n".to_owned()
        )
    );
    let (status, _, why) = typed(b"short", b"short");
    assert_eq!(status, 1);
    assert!(why.contains("8 at least"), "{why}");
}
