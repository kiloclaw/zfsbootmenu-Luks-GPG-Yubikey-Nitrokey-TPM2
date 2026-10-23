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
    let relay = std::thread::spawn(move || relay(&mut master, &mut terminal));

    program.write_all(b"Booting\r\n").unwrap();
    let mut shown = [0_u8; 9];
    user.read_exact(&mut shown).unwrap();
    user.write_all(b"y").unwrap();
    let mut typed = [0_u8; 1];
    program.read_exact(&mut typed).unwrap();
    drop(program);

    relay.join().unwrap().unwrap();
    assert_eq!((&shown, &typed), (b"Booting\r\n", b"y"));
}

#[test]
fn the_relay_ends_when_the_console_closes() {
    let (mut master, _program) = UnixStream::pair().unwrap();
    let (mut terminal, user) = UnixStream::pair().unwrap();
    drop(user);

    relay(&mut master, &mut terminal).unwrap();
}
