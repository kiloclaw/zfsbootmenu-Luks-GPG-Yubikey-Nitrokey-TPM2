//! The console without a display relays on the kernel console, and the
//! hand-over is asked for and waited on.

use std::io::{Read as _, Write as _};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;

use pretty_assertions::assert_eq;
use zbm_testkit::TempDir;

use super::*;

/// The places of a machine without DRM cards or keyboards, in `dir`.
fn places(dir: &TempDir) -> Places {
    Places {
        sys: dir.join("sys"),
        dev: dir.join("dev"),
        base: dir.join("zfsbootmenu"),
        kmsg: dir.write("kmsg", ""),
    }
}

fn file(stream: UnixStream) -> File {
    File::from(OwnedFd::from(stream))
}

/// Runs the console on `settings` without a display, while the program
/// writes `hello` and goes; returns what the kernel console showed.
fn without_display(dir: &TempDir, settings: Settings) -> String {
    let places = places(dir);
    std::fs::create_dir_all(&places.base).unwrap();
    std::fs::write(places.base.join("console"), "77\n").unwrap();
    let (master, mut program) = UnixStream::pair().unwrap();
    let (terminal, mut user) = UnixStream::pair().unwrap();
    let vt = dir.join("tty1");
    let console = std::thread::spawn(move || {
        let (mut master, mut terminal) = (file(master), file(terminal));
        let mut asked = false;
        let terminals = Terminals {
            master: &mut master,
            terminal: &mut terminal,
            vt: &vt,
        };
        let served = serve_on(
            &settings,
            &places,
            &mut |_| {
                asked = true;
                Err(io::Error::other("no font"))
            },
            terminals,
            Arc::new(AtomicBool::new(false)),
        );
        served.map(|()| asked)
    });

    program.write_all(b"hello").unwrap();
    let mut shown = [0_u8; 12];
    user.read_exact(&mut shown).unwrap();
    drop(program);
    let asked = console.join().unwrap().unwrap();

    assert!(!asked, "no display, so no font");
    assert!(!dir.join("zfsbootmenu/console").exists());
    String::from_utf8_lossy(&shown).into_owned()
}

#[test]
fn without_a_card_the_console_relays_on_the_kernel_console() {
    let dir = TempDir::new("zbm-console-serve");

    let shown = without_display(&dir, Settings::default());

    assert_eq!(shown, "\x1b[H\x1b[J\nhello");
    let log = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert_eq!(
        log,
        "<4>ZFSBootMenu: console: no DRM card; going on on the kernel console\n"
    );
}

#[test]
fn a_card_that_cannot_be_opened_is_said_so() {
    let dir = TempDir::new("zbm-console-serve");
    let settings = Settings {
        card: Some(dir.join("dri/card9")),
        mirror: None,
    };

    without_display(&dir, settings);

    let log = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert!(log.contains("going on on the kernel console"), "{log}");
}

#[test]
fn the_mirror_gets_lines_of_their_own() {
    let dir = TempDir::new("zbm-console-serve");
    let mirror = dir.write("mirror", "");
    let settings = Settings {
        card: None,
        mirror: Some(mirror.clone()),
    };

    settings.mirror_line(RELEASED);
    Settings::default().mirror_line(RELEASED);

    assert_eq!(
        std::fs::read_to_string(&mirror).unwrap(),
        format!("\x0c{RELEASED}\n")
    );
    assert_eq!(Places::system().kmsg, PathBuf::from("/dev/kmsg"));
    assert!(open_mirror(&mirror).is_some());
    assert!(open_mirror(&dir.join("none/mirror")).is_none());
}

#[test]
fn the_hand_over_is_asked_for_and_waited_on() {
    let dir = TempDir::new("zbm-console-release");
    let base = dir.join("zfsbootmenu");
    std::fs::create_dir_all(&base).unwrap();
    let timeout = Duration::from_secs(5);

    // No console: nothing to do.
    assert_eq!(release(&base, timeout).unwrap(), Released::Done);

    // A console that is gone.
    std::fs::write(base.join("console"), "4194000\n").unwrap();
    assert_eq!(release(&base, timeout).unwrap(), Released::Done);
    assert!(!base.join("console").exists());

    // This process plays the console, which removes its record once asked.
    let requested = crate::release::on_request().unwrap();
    std::fs::write(base.join("console"), format!("{}\n", std::process::id())).unwrap();
    let record = base.join("console");
    let console = std::thread::spawn(move || {
        while !requested.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::remove_file(record).unwrap();
    });
    assert_eq!(release(&base, timeout).unwrap(), Released::Done);
    console.join().unwrap();

    // A console that never answers.
    std::fs::write(base.join("console"), format!("{}\n", std::process::id())).unwrap();
    assert_eq!(
        release(&base, Duration::from_millis(30)).unwrap(),
        Released::Late
    );
}

/// A writer that takes at most `room` bytes, then would block.
struct Narrow {
    written: Vec<u8>,
    room: usize,
}

impl io::Write for Narrow {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.room == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let taken = bytes.len().min(self.room);
        self.written.extend_from_slice(&bytes[..taken]);
        self.room -= taken;
        Ok(taken)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_mirror_ends_with_the_screen_shown_when_the_line_was_full() {
    let mut mirror = Mirror::new(Narrow {
        written: Vec::new(),
        room: 3,
    });

    mirror.show("one");
    mirror.show("two");
    mirror.show("three");
    assert_eq!(mirror.get_ref().written, b"\x0con");
    // The line takes more: the screen begun ends, then the latest follows.
    mirror.out.room = 100;
    mirror.show("three");
    assert_eq!(mirror.get_ref().written, b"\x0cone\x0cthree");
    mirror.show("three");
    assert_eq!(mirror.get_ref().written, b"\x0cone\x0cthree");
}
