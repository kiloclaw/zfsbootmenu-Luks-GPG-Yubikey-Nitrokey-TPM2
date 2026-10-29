//! The relay between the programs and the kernel console.

use std::os::unix::net::UnixStream;
use std::sync::Arc;

use pretty_assertions::assert_eq;
use zbm_display::grid::Grid;
use zbm_display::psf::Font;

use super::*;

#[test]
fn the_relay_carries_both_ways_until_the_programs_are_gone() {
    let (mut master, mut program) = UnixStream::pair().unwrap();
    let (mut terminal, mut user) = UnixStream::pair().unwrap();
    let relay = std::thread::spawn(move || relay(&mut master, &mut terminal, None, None));

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
        relay(&mut master, &mut terminal, None, None).unwrap(),
        Relayed::Hangup
    );
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

    relay(&mut master, &mut terminal, None, None).unwrap();

    assert_eq!(&flood.join().unwrap(), b"\x03");
    drop(terminal);
    drain.join().unwrap();
}

#[test]
fn the_relay_ends_when_the_display_is_asked_for_again() {
    let (mut master, _program) = UnixStream::pair().unwrap();
    let (mut terminal, _user) = UnixStream::pair().unwrap();
    let (control, _wake) = Control::new().unwrap();
    let asking = Arc::clone(&control);
    let relay = std::thread::spawn(move || relay(&mut master, &mut terminal, Some(&control), None));

    std::thread::sleep(Duration::from_millis(50));
    asking.ask_reattach();

    assert_eq!(relay.join().unwrap().unwrap(), Relayed::Reattach);
    assert!(
        asking.is_reattach_asked(),
        "asked until the display is shown"
    );
    asking.shown("us", "font");
    assert!(!asking.is_reattach_asked());
}

#[test]
fn the_console_s_terminal_kept_follows_what_is_relayed() {
    let (mut master, mut program) = UnixStream::pair().unwrap();
    let (mut terminal, mut user) = UnixStream::pair().unwrap();
    let relay = std::thread::spawn(move || {
        const TER_V14B: &[u8] = include_bytes!("../../../../legacy/zfsbootmenu/fonts/ter-v14b.psf");
        let grid = Grid::new(Font::parse(TER_V14B).unwrap(), 1);
        let mut kept = Console::new(grid, (20 * 8, 3 * 14));
        let relayed = relay(&mut master, &mut terminal, None, Some(&mut kept));
        (relayed.unwrap(), kept.text())
    });

    program.write_all(b"kexec failed\r\n\x1b[6n").unwrap();
    let mut shown = [0_u8; 18];
    user.read_exact(&mut shown).unwrap();
    drop(program);

    let (relayed, text) = relay.join().unwrap();
    assert_eq!(relayed, Relayed::Hangup);
    assert_eq!(text.lines().next(), Some("kexec failed"));
}

#[test]
fn saved_modes_come_back_after_a_raw_terminal() {
    let (_master, terminal) = crate::host::open_pty().unwrap();
    let saved = SavedModes::of(&terminal).unwrap();
    let mut raw = termios::tcgetattr(&terminal).unwrap();
    raw.make_raw();
    termios::tcsetattr(&terminal, OptionalActions::Now, &raw).unwrap();
    let cooked = termios::LocalModes::ECHO | termios::LocalModes::ICANON;
    assert!(
        !termios::tcgetattr(&terminal)
            .unwrap()
            .local_modes
            .intersects(cooked)
    );

    saved.restore().unwrap();

    assert!(
        termios::tcgetattr(&terminal)
            .unwrap()
            .local_modes
            .contains(cooked)
    );
    assert!(SavedModes::of(std::fs::File::open("/dev/null").unwrap()).is_none());
}
