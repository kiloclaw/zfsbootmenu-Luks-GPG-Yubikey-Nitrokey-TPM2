//! The real host on sockets and a pseudo-terminal, and the relay.

use std::io::Cursor;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::*;

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
    let release = Arc::new(AtomicBool::new(false));
    let keyboards = Keyboards::from_files(vec![File::from(OwnedFd::from(keyboard))]);
    let mut host = RealHost::new(master, keyboards, Arc::clone(&release));
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
    };
    assert_eq!(host.wait(timeout).unwrap(), Input::Keys(vec![a]));
    host.send(b"in").unwrap();
    let mut sent = [0_u8; 2];
    program.read_exact(&mut sent).unwrap();
    assert_eq!(&sent, b"in");

    release.store(true, Ordering::SeqCst);
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
    let requested = Arc::new(AtomicBool::new(false));
    let mut switched = Vec::new();
    let mut host = RealHost::new(master, keyboards, Arc::new(AtomicBool::new(false))).with_layouts(
        Arc::clone(&requested),
        Box::new(|keyboards: &mut Keyboards| switched.push(keyboards.layout().to_owned())),
    );
    let timeout = Duration::from_millis(10);

    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    requested.store(true, Ordering::SeqCst);
    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    assert_eq!(host.wait(timeout).unwrap(), Input::Nothing);
    drop(host);

    assert_eq!(switched, ["us"]);
}

#[test]
fn the_relay_carries_both_ways_until_the_programs_are_gone() {
    let (mut master, mut program) = UnixStream::pair().unwrap();
    let (mut terminal, mut user) = UnixStream::pair().unwrap();
    let relay = std::thread::spawn(move || relay(&mut master, &mut terminal, None));

    program.write_all(b"Booting\r\n").unwrap();
    let mut shown = [0_u8; 9];
    user.read_exact(&mut shown).unwrap();
    user.write_all(b"y").unwrap();
    let mut typed = [0_u8; 1];
    program.read_exact(&mut typed).unwrap();
    drop(program);

    assert_eq!(relay.join().unwrap().unwrap(), Relayed::Hangup);
    assert_eq!((&shown, &typed), (b"Booting\r\n", b"y"));
}

#[test]
fn the_relay_ends_when_the_console_closes() {
    let (mut master, _program) = UnixStream::pair().unwrap();
    let (mut terminal, user) = UnixStream::pair().unwrap();
    drop(user);

    assert_eq!(
        relay(&mut master, &mut terminal, None).unwrap(),
        Relayed::Hangup
    );
}

#[test]
fn keys_come_before_a_flood_of_output() {
    let (master, mut program) = UnixStream::pair().unwrap();
    let (keyboard, mut typing) = UnixStream::pair().unwrap();
    let keyboards = Keyboards::from_files(vec![File::from(OwnedFd::from(keyboard))]);
    let mut host = RealHost::new(master, keyboards, Arc::new(AtomicBool::new(false)));
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
    let mut host = RealHost::new(master, keyboards, Arc::new(AtomicBool::new(false)));
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
    let mut host = RealHost::new(master, keyboards, Arc::new(AtomicBool::new(false)));
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
fn the_relay_serves_the_user_before_a_flood() {
    let (mut master, mut program) = UnixStream::pair().unwrap();
    let (mut terminal, mut user) = UnixStream::pair().unwrap();
    let flood = std::thread::spawn(move || {
        let mut typed = [0_u8; 1];
        program.set_nonblocking(true).unwrap();
        loop {
            let _ = program.write(&[b'y'; 1024]);
            if program.read(&mut typed).is_ok() {
                break typed;
            }
        }
    });
    let drain = std::thread::spawn(move || {
        user.write_all(b"\x03").unwrap();
        let mut sink = vec![0_u8; 64 * 1024];
        while user.read(&mut sink).is_ok_and(|length| length > 0) {}
    });

    relay(&mut master, &mut terminal, None).unwrap();

    assert_eq!(&flood.join().unwrap(), b"\x03");
    drop(terminal);
    drain.join().unwrap();
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
fn the_relay_ends_when_the_display_is_asked_for_again() {
    let (mut master, _program) = UnixStream::pair().unwrap();
    let (mut terminal, _user) = UnixStream::pair().unwrap();
    let asked = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&asked);
    let relay = std::thread::spawn(move || relay(&mut master, &mut terminal, Some(&flag)));

    std::thread::sleep(Duration::from_millis(50));
    asked.store(true, Ordering::SeqCst);

    assert_eq!(relay.join().unwrap().unwrap(), Relayed::Reattach);
    assert!(!asked.load(Ordering::SeqCst));
}
