//! The real host on sockets and a pseudo-terminal, and the relay.

use std::io::Cursor;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

use pretty_assertions::assert_eq;
use rustix::termios::OptionalActions;
use zbm_app::console::Reply;
use zbm_display::keys::{Key, KeyPress};

use super::*;

/// What a host does when nothing asks it anything.
fn control() -> Arc<Control> {
    Control::new().unwrap().0
}

/// A reader that fails with `error`.
struct Failing(i32);

impl Read for Failing {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::from_raw_os_error(self.0))
    }
}

#[test]
fn output_end_of_file_and_eio_mean_the_programs_are_gone() {
    assert_eq!(
        read_output(&mut Cursor::new(b"ab")).unwrap(),
        Input::Output(b"ab".to_vec())
    );
    assert_eq!(read_output(&mut Cursor::new(b"")).unwrap(), Input::Hangup);
    assert_eq!(read_output(&mut Failing(5)).unwrap(), Input::Hangup);
    assert_eq!(read_output(&mut Failing(4)).unwrap(), Input::Nothing);
    assert!(read_output(&mut Failing(9)).is_err());
}

#[test]
fn a_pseudo_terminal_gets_its_size_and_hangs_up() {
    let (mut master, mut slave) = open_pty().unwrap();
    let size = WindowSize {
        num_lines: 25,
        num_cols: 80,
        cell_width: 8,
        cell_height: 16,
    };

    set_size(&master, &size).unwrap();
    let winsize = termios::tcgetwinsize(&slave).unwrap();
    assert_eq!(
        (
            winsize.ws_row,
            winsize.ws_col,
            winsize.ws_xpixel,
            winsize.ws_ypixel
        ),
        (25, 80, 640, 400)
    );
    assert!(set_size(File::open("/dev/null").unwrap(), &size).is_err());

    slave.write_all(b"x").unwrap();
    assert_eq!(
        read_output(&mut master).unwrap(),
        Input::Output(b"x".to_vec())
    );
    drop(slave);
    assert_eq!(read_output(&mut master).unwrap(), Input::Hangup);
}

/// The record of an input event: key `code`, pressed (1) or released (0).
fn event(code: u16, value: i32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 16];
    bytes.extend_from_slice(&1_u16.to_ne_bytes());
    bytes.extend_from_slice(&code.to_ne_bytes());
    bytes.extend_from_slice(&value.to_ne_bytes());
    bytes
}

#[test]
fn the_host_waits_on_the_programs_the_keyboards_and_the_release() {
    let (master, mut program) = UnixStream::pair().unwrap();
    let (keyboard, mut typing) = UnixStream::pair().unwrap();
    let control = control();
    let keyboards = Keyboards::from_files(vec![File::from(OwnedFd::from(keyboard))]);
    let mut host = RealHost::new(master, keyboards, Arc::clone(&control));
    let timeout = Duration::from_millis(10);

    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    program.write_all(b"out").unwrap();
    assert_eq!(host.wait(timeout).unwrap(), Input::Output(b"out".to_vec()));
    typing
        .write_all(&[event(30, 1), event(30, 0)].concat())
        .unwrap();
    let a = KeyPress {
        key: Key::Char('a'),
        shift: false,
        ctrl: false,
        alt: false,
        logo: false,
        keypad: false,
    };
    assert_eq!(host.wait(timeout).unwrap(), Input::Keys(vec![a]));
    host.send(b"in").unwrap();
    let mut sent = [0_u8; 2];
    program.read_exact(&mut sent).unwrap();
    assert_eq!(&sent, b"in");

    control.ask_release();
    assert_eq!(host.wait(timeout).unwrap(), Input::Release);
    assert!(format!("{host:?}").starts_with("RealHost"));
    drop(program);
    let mut master = host.into_master();
    assert_eq!(read_output(&mut master).unwrap(), Input::Hangup);
}

#[test]
fn a_layout_asked_for_reaches_the_keyboards() {
    let (master, _program) = UnixStream::pair().unwrap();
    let (keyboard, _typing) = UnixStream::pair().unwrap();
    let keyboards = Keyboards::from_files(vec![File::from(OwnedFd::from(keyboard))]);
    let control = control();
    let mut switched = Vec::new();
    let mut host = RealHost::new(master, keyboards, Arc::clone(&control)).with_layouts(Box::new(
        |keyboards: &mut Keyboards, change: Change<'_>| {
            switched.push((keyboards.layout().to_owned(), format!("{change:?}")));
        },
    ));
    let timeout = Duration::from_millis(10);

    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    control.ask_layout("de");
    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    // The keyboards kept their layout: the image has none of that name.
    assert_eq!(
        control.replies().0,
        Some(Reply::NoSuchLayout("de".to_owned()))
    );
    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    // Super+Space asks for the next one.
    host.next_layout();
    // Not a terminal, nor a prompt held: no secret prompt.
    assert!(!host.is_secret());
    control.set_secrets(1);
    assert!(host.is_secret());
    assert_eq!(host.pending_accent(), None);
    drop(host);

    let us = "us".to_owned();
    assert_eq!(
        switched,
        [
            (us.clone(), "Requested(\"de\")".to_owned()),
            (us, "Next".to_owned())
        ]
    );
}

#[test]
fn scroll_lock_holds_the_programs_output() {
    let (master, mut program) = UnixStream::pair().unwrap();
    let (keyboard, mut typing) = UnixStream::pair().unwrap();
    let keyboards = Keyboards::from_files(vec![File::from(OwnedFd::from(keyboard))]);
    let mut host = RealHost::new(master, keyboards, control());
    let timeout = Duration::from_millis(10);

    typing
        .write_all(&[event(70, 1), event(70, 0)].concat())
        .unwrap();
    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    program.write_all(b"held").unwrap();
    for _ in 0..3 {
        assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    }
    // Typed text still goes out while output is held.
    host.send(b"q").unwrap();
    let mut sent = [0_u8; 1];
    program.read_exact(&mut sent).unwrap();
    typing
        .write_all(&[event(70, 1), event(70, 0)].concat())
        .unwrap();
    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    assert_eq!(host.wait(timeout).unwrap(), Input::Output(b"held".to_vec()));
}

/// The record of a relative movement of `value` on axis `code`, as a mouse
/// sends it (`EV_REL`).
fn movement(code: u16, value: i32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 16];
    bytes.extend_from_slice(&2_u16.to_ne_bytes());
    bytes.extend_from_slice(&code.to_ne_bytes());
    bytes.extend_from_slice(&value.to_ne_bytes());
    bytes
}

#[test]
fn a_keyboard_that_moves_a_mouse_holds_up_no_output() {
    let (master, mut program) = UnixStream::pair().unwrap();
    let (keyboard, mut typing) = UnixStream::pair().unwrap();
    let keyboards = Keyboards::from_files(vec![File::from(OwnedFd::from(keyboard))]);
    let mut host = RealHost::new(master, keyboards, control());
    let timeout = Duration::from_millis(10);

    program.write_all(b"out").unwrap();
    typing
        .write_all(&[movement(0, 5), movement(1, -3)].concat())
        .unwrap();

    assert_eq!(host.wait(timeout).unwrap(), Input::Output(b"out".to_vec()));
}

#[test]
fn keys_come_before_a_flood_of_output() {
    let (master, mut program) = UnixStream::pair().unwrap();
    let (keyboard, mut typing) = UnixStream::pair().unwrap();
    let keyboards = Keyboards::from_files(vec![File::from(OwnedFd::from(keyboard))]);
    let mut host = RealHost::new(master, keyboards, control());
    let timeout = Duration::from_millis(10);

    program.write_all(&[b'y'; 4096]).unwrap();
    typing
        .write_all(&[event(29, 1), event(46, 1)].concat())
        .unwrap();

    let Input::Keys(keys) = host.wait(timeout).unwrap() else {
        panic!("the keys were not served first");
    };
    assert_eq!(keys.len(), 1);
    assert!(keys[0].ctrl);
    assert!(matches!(host.wait(timeout).unwrap(), Input::Output(_)));
}

#[test]
fn typing_to_a_program_that_reads_nothing_does_not_block() {
    let (master, mut program) = UnixStream::pair().unwrap();
    let (keyboard, typing) = UnixStream::pair().unwrap();
    let keyboards = Keyboards::from_files(vec![File::from(OwnedFd::from(keyboard))]);
    let mut host = RealHost::new(master, keyboards, control());
    let timeout = Duration::from_millis(10);

    // Far more than a socket's buffer: the rest waits in the host.
    let chunk = vec![b'k'; 64 * 1024];
    let mut sends = 0;
    while host.outgoing.is_empty() && sends < 1000 {
        host.send(&chunk).unwrap();
        sends += 1;
    }
    assert!(!host.outgoing.is_empty());
    program.write_all(b"out").unwrap();
    assert_eq!(host.wait(timeout).unwrap(), Input::Output(b"out".to_vec()));

    program.set_nonblocking(true).unwrap();
    let mut taken = 0;
    let mut sink = vec![0_u8; 64 * 1024];
    for _ in 0..10_000 {
        match program.read(&mut sink) {
            Ok(length) => taken += length,
            Err(_) if host.outgoing.is_empty() => break,
            Err(_) => {
                let _ = host.wait(timeout).unwrap();
            }
        }
    }
    assert!(host.outgoing.is_empty());
    assert!(taken > 64 * 1024 && taken <= sends * chunk.len());
    drop(typing);
}

#[test]
fn a_keyboard_that_goes_away_leaves_the_host_running() {
    let (master, mut program) = UnixStream::pair().unwrap();
    let (keyboard, typing) = UnixStream::pair().unwrap();
    let keyboards = Keyboards::from_files(vec![File::from(OwnedFd::from(keyboard))]);
    let mut host = RealHost::new(master, keyboards, control());
    let timeout = Duration::from_millis(10);

    drop(typing);

    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    program.write_all(b"still").unwrap();
    assert_eq!(
        host.wait(timeout).unwrap(),
        Input::Output(b"still".to_vec())
    );
}

#[test]
fn queued_bytes_are_written_as_the_other_side_takes_them() {
    let mut outgoing = Outgoing::default();
    let mut taken = Vec::new();

    outgoing.push(b"abc");
    outgoing.push(&vec![b'x'; OUTGOING]);
    outgoing.flush(&mut taken).unwrap();
    assert_eq!(taken.len(), OUTGOING);
    assert!(outgoing.is_empty());
    outgoing.push(b"y");
    assert!(outgoing.flush(&mut Failing(9)).is_err());
    outgoing.flush(&mut taken).unwrap();

    outgoing.push(b"z");
    let mut full: &mut [u8] = &mut [];
    outgoing.flush(&mut full).unwrap();
    assert!(!outgoing.is_empty());
}

impl Write for Failing {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::from_raw_os_error(self.0))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_terminal_left_raw_gets_its_modes_back() {
    let (_master, slave) = open_pty().unwrap();
    let modes = Modes::save(&slave).unwrap();
    let mut raw = termios::tcgetattr(&slave).unwrap();
    raw.make_raw();
    termios::tcsetattr(&slave, OptionalActions::Now, &raw).unwrap();

    modes.restore().unwrap();

    let restored = termios::tcgetattr(&slave).unwrap();
    assert!(restored.local_modes.contains(termios::LocalModes::ISIG));
    assert!(restored.local_modes.contains(termios::LocalModes::ECHO));
    assert!(Modes::save(File::open("/dev/null").unwrap()).is_none());
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "a request comes from another thread after a while"
)]
fn a_host_woken_by_its_requests_and_devices_sleeps_long() {
    let (master, _program) = UnixStream::pair().unwrap();
    let (keyboard, _typing) = UnixStream::pair().unwrap();
    let (devices, mut appeared) = UnixStream::pair().unwrap();
    devices.set_nonblocking(true).unwrap();
    let (control, wake) = Control::new().unwrap();
    let keyboards = Keyboards::from_files(vec![File::from(OwnedFd::from(keyboard))]);
    let host = RealHost::new(master, keyboards, Arc::clone(&control)).with_wake(wake);
    // Without a watch on the devices, it still looks for keyboards.
    assert_eq!(host.idle(), crate::run::TICK);
    let mut host = host.with_device_watch(File::from(OwnedFd::from(devices)));
    assert_eq!(host.idle(), IDLE);

    // A device appeared: keyboards are looked for on every tick a while.
    appeared.write_all(b"event").unwrap();
    assert_eq!(host.wait(IDLE).unwrap(), Input::Nothing);
    assert_eq!(host.idle(), crate::run::TICK);
    host.rescan_until = Some(Instant::now());
    assert_eq!(host.idle(), IDLE);

    // A request wakes it at once, long before the time runs out.
    let asking = std::thread::spawn({
        let control = Arc::clone(&control);
        move || {
            std::thread::sleep(Duration::from_millis(50));
            control.ask_release();
        }
    });
    let start = Instant::now();
    assert_eq!(host.wait(IDLE).unwrap(), Input::Release);
    assert!(start.elapsed() < Duration::from_secs(5));
    asking.join().unwrap();
    // Asked again before the next wait: it is taken at once, with its
    // wake-up.
    control.ask_release();
    assert_eq!(host.wait(IDLE).unwrap(), Input::Release);
    // The wake-up was taken: the next wait does not return at once.
    control.released();
    let start = Instant::now();
    assert_eq!(
        host.wait(Duration::from_millis(50)).unwrap(),
        Input::Nothing
    );
    assert!(start.elapsed() >= Duration::from_millis(40));
}

#[test]
fn the_state_directory_keeps_the_scrollback_and_screenshots() {
    let dir = zbm_testkit::TempDir::new("zbm-console-host");
    let (master, _program) = UnixStream::pair().unwrap();
    let keyboards = Keyboards::from_files(Vec::new());
    let mut host = RealHost::new(master, keyboards, control()).with_state(dir.path());

    let status = host.status().unwrap();
    assert_eq!(status.layout, "us");
    assert_eq!(status.locks, crate::screen::Locks::default());
    assert!(status.clock.is_some());
    host.ring();
    let shot = host.save_screenshot(b"$\n", b"PNG").unwrap();
    assert_eq!(shot, dir.join("screenshot-1.png"));
    let saved = host.save_scrollback(b"$ zpool status\n").unwrap();
    assert_eq!(saved, dir.join("console-scrollback.txt"));
    assert_eq!(std::fs::read(&saved).unwrap(), b"$ zpool status\n");
    std::fs::remove_dir_all(dir.path()).unwrap();
    assert!(host.save_scrollback(b"x").is_err());
}

#[test]
fn a_host_without_a_state_directory_saves_nothing() {
    let (master, _program) = UnixStream::pair().unwrap();
    let keyboards = Keyboards::from_files(Vec::new());
    let mut host = RealHost::new(master, keyboards, control());
    let error = host.save_scrollback(b"x").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    let error = host.save_screenshot(b"x", b"y").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
}

#[test]
fn a_font_request_is_taken_and_answered() {
    let dir = zbm_testkit::TempDir::new("zbm-console-host");
    let (master, _program) = open_pty().unwrap();
    let keyboards = Keyboards::from_files(Vec::new());
    let control = control();
    let mut grids = Vec::new();
    let mut fonts = |pixels: (u32, u32), height: Option<u16>| {
        grids.push((pixels, height));
        Err(io::Error::other("no font"))
    };
    let mut host = RealHost::new(master, keyboards, Arc::clone(&control))
        .with_layouts(Box::new(|_: &mut Keyboards, _: Change<'_>| {}))
        .with_state(dir.path())
        .with_fonts(&mut fonts);
    let state = zbm_state::State::new(&zbm_platform::RealFiles, dir.path());
    let timeout = Duration::from_millis(10);

    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    assert_eq!(host.font_request(), None);
    control.ask_font(Zoom::Bigger);
    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    assert_eq!(host.font_request(), Some(Zoom::Bigger));
    assert_eq!(host.font_request(), None);
    assert!(host.grid((640, 480), Some(32)).is_none());
    // Answered: the font recorded, the reply given, the size told.
    let size = WindowSize {
        num_lines: 25,
        num_cols: 80,
        cell_width: 8,
        cell_height: 16,
    };
    host.font_shown("ter-v16b.psf 8x16, 80x25", &size).unwrap();
    assert_eq!(
        control.replies().1.as_deref(),
        Some("ter-v16b.psf 8x16, 80x25")
    );
    assert_eq!(
        state.console_font().as_deref(),
        Some("ter-v16b.psf 8x16, 80x25")
    );
    drop(host);
    assert_eq!(grids, [((640, 480), Some(32))]);
}

#[test]
fn the_client_is_read_and_written_and_found_gone() {
    use zbm_display::channel::{FromClient, FromConsole, VERSION};
    use zbm_sys::socket::Connection;

    let (master, _program) = UnixStream::pair().unwrap();
    let keyboards = Keyboards::from_files(Vec::new());
    let (console_end, menu) = Connection::pair().unwrap();
    let mut switched = Vec::new();
    let mut notes = Vec::new();
    let mut host = RealHost::new(master, keyboards, control())
        .with_layouts(Box::new(|_: &mut Keyboards, change: Change<'_>| {
            switched.push(format!("{change:?}"));
        }))
        .with_client(&console_end)
        .with_notes(|text| notes.push(text.to_owned()));
    host.note("said");
    let timeout = Duration::from_millis(10);

    menu.send(&FromClient::Hello { version: VERSION }.encode())
        .unwrap();
    assert_eq!(
        host.wait(timeout).unwrap(),
        Input::Client(FromClient::Hello { version: VERSION })
    );
    // What is not a message is dropped.
    menu.send(&[99]).unwrap();
    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);

    let key = FromConsole::Key {
        press: KeyPress::new(Key::Char('a')),
        layout: host.layout_name(),
    };
    host.send_client(&key);
    let packet = menu.receive(Some(timeout)).unwrap().unwrap();
    assert_eq!(FromConsole::decode(&packet), Some(key));
    host.switch_layout("de");

    // A client that reads nothing loses what does not fit, and holds up
    // nothing.
    for _ in 0..10_000 {
        host.send_client(&FromConsole::Bell);
    }
    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);

    drop(menu);
    assert_eq!(host.wait(timeout).unwrap(), Input::ClientGone);
    // Said once; the client is not waited on again.
    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    host.send_client(&FromConsole::Bell);
    drop(host);
    assert_eq!(switched, ["Requested(\"de\")"]);
    assert_eq!(notes, ["said"]);
}

#[test]
fn a_client_that_cannot_be_written_is_gone() {
    use zbm_display::channel::FromConsole;
    use zbm_sys::socket::Connection;

    let (master, _program) = UnixStream::pair().unwrap();
    let (console_end, menu) = Connection::pair().unwrap();
    let mut host = RealHost::new(master, Keyboards::from_files(Vec::new()), control())
        .with_client(&console_end);
    drop(menu);

    host.send_client(&FromConsole::Bell);
    assert_eq!(host.wait(Duration::ZERO).unwrap(), Input::ClientGone);
}
