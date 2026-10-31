//! The serial terminal on a pseudo-terminal, whose other side stands for
//! the person at the serial line.

use std::io::{Read as _, Write as _};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use pretty_assertions::assert_eq;
use zbm_display::grid::Grid;
use zbm_display::input::Keyboards;
use zbm_display::keys::Key;
use zbm_display::psf::Font;

use super::*;
use crate::control::Control;
use crate::host::open_pty;
use crate::run::{Host as _, Input};
use crate::scheme::LINUX;

/// The image's Terminus 8×14.
const TER_V14B: &[u8] = include_bytes!("../../../../../legacy/zfsbootmenu/fonts/ter-v14b.psf");

/// A serial line: the person's side, and the path of the console's.
fn line() -> (File, std::path::PathBuf) {
    let (person, console) = open_pty().unwrap();
    let path = std::fs::read_link(format!(
        "/proc/self/fd/{}",
        std::os::fd::AsRawFd::as_raw_fd(&console)
    ))
    .unwrap();
    // The console's side is opened again by the port, by its path.
    std::mem::forget(console);
    rustix::io::ioctl_fionbio(&person, true).unwrap();
    (person, path)
}

/// What the person's side got so far.
fn received(person: &mut File) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    while let Ok(length) = person.read(&mut buffer) {
        if length == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..length]);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn the_port_reads_keys_writes_the_screen_and_gives_the_terminal_its_modes_back() {
    let (mut person, path) = line();
    let mut port = SerialPort::open(&path, (20, 3), &LINUX).unwrap();
    assert_eq!(port.size(), (20, 3));
    assert!(port.is_read());
    // Raw: keys come as typed, not after a line.
    let modes = termios::tcgetattr(port.fd()).unwrap();
    assert!(!modes.local_modes.contains(termios::LocalModes::ICANON));

    person.write_all(b"b\x1b[A").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    let keys = port.read_keys(Instant::now());
    assert_eq!(
        keys,
        [KeyPress::new(Key::Char('b')), KeyPress::new(Key::Up)]
    );

    let mut console = Console::new(
        Grid::new(Font::parse(TER_V14B).unwrap(), 1),
        (20 * 8, 3 * 14),
    );
    console.feed(b"boot menu");
    port.show(&console);
    port.flush();
    let screen = received(&mut person);
    assert!(screen.contains("\x1b[2J"), "{screen:?}");
    assert!(screen.contains("boot menu"), "{screen:?}");
    assert!(!port.is_pending());
    assert!(port.idle(Instant::now(), Duration::from_secs(1)) <= Duration::from_secs(1));
    assert!(format!("{port:?}").starts_with("SerialPort"));

    drop(port);
    let console_side = File::open(&path).unwrap();
    let modes = termios::tcgetattr(&console_side).unwrap();
    assert!(modes.local_modes.contains(termios::LocalModes::ICANON));
}

#[test]
fn ctrl_l_writes_the_whole_screen_again() {
    let (mut person, path) = line();
    let mut port = SerialPort::open(&path, (20, 3), &LINUX).unwrap();
    let console = Console::new(
        Grid::new(Font::parse(TER_V14B).unwrap(), 1),
        (20 * 8, 3 * 14),
    );
    port.show(&console);
    let _ = received(&mut person);
    person.write_all(b"\x0c").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    let keys = port.read_keys(Instant::now());
    assert_eq!(keys.len(), 1);
    port.show(&console);
    assert!(received(&mut person).contains("\x1b[2J"));
}

#[test]
fn the_host_says_the_serial_keys_and_the_terminal_s_size() {
    let (mut person, path) = line();
    let (master, _program) = UnixStream::pair().unwrap();
    let (keyboard, _typing) = UnixStream::pair().unwrap();
    let keyboards = Keyboards::from_files(vec![File::from(OwnedFd::from(keyboard))]);
    let control = Control::new().unwrap().0;
    let port = SerialPort::open(&path, (80, 25), &LINUX).unwrap();
    let mut host = RealHost::new(master, keyboards, Arc::clone(&control)).with_serial(port);
    assert_eq!(host.serial_size(), Some((80, 25)));

    person.write_all(b"x").unwrap();
    let input = host.wait(Duration::from_millis(200)).unwrap();
    assert_eq!(
        input,
        Input::SerialKeys(vec![KeyPress::new(Key::Char('x'))])
    );
    // ESC alone is the Escape key once its time ran out.
    person.write_all(b"\x1b").unwrap();
    let mut keys = Vec::new();
    for _ in 0..20 {
        if let Input::SerialKeys(got) = host.wait(Duration::from_millis(50)).unwrap() {
            keys = got;
            break;
        }
    }
    assert_eq!(keys, [KeyPress::new(Key::Escape)]);
    assert!(host.idle() <= crate::host::IDLE);
}
