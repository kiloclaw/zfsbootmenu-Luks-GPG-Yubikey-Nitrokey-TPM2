//! What a program that draws on the display itself needs from the kernel:
//! the virtual terminal's display and keyboard modes, which it takes over
//! while it draws and gives back before it ends (`zbm-display`), and a wait
//! on several input devices at once.
//!
//! The waits are `rustix`'s `poll`. The terminal and input device requests
//! are `ioctl`s that `rustix` has no wrapper for: they go through its typed
//! `ioctl`, with the opcodes built for the target by `rustix::ioctl::opcode`,
//! and each states what the kernel reads or writes.

use std::ffi::c_int;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::ioctl::{Getter, IntegerSetter, NoArg, Opcode, Setter, ioctl, opcode};

/// `KDSETMODE`: text or graphics on a virtual terminal (`linux/kd.h`),
/// numbered before `_IOC` and the same everywhere.
const KDSETMODE: Opcode = 0x4B3A;

/// `KDGKBMODE`: the keyboard mode of a virtual terminal.
const KDGKBMODE: Opcode = 0x4B44;

/// `KDSKBMODE`: sets the keyboard mode of a virtual terminal.
const KDSKBMODE: Opcode = 0x4B45;

/// What a virtual terminal shows: its text, or what a program draws.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayMode {
    /// `KD_TEXT`: the kernel console draws the terminal's text.
    Text,
    /// `KD_GRAPHICS`: the kernel console draws nothing, not even its
    /// cursor, until the mode is text again.
    Graphics,
}

/// The keyboard mode of a virtual terminal (`K_RAW`, `K_XLATE`,
/// `K_MEDIUMRAW`, `K_UNICODE` or `K_OFF`), as the kernel numbers it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyboardMode(pub i32);

impl KeyboardMode {
    /// `K_OFF`: keys reach no program through the terminal, only through
    /// the input devices.
    pub const OFF: KeyboardMode = KeyboardMode(4);
}

/// `TIOCCONS`: writes to `/dev/console` go to this terminal, as
/// `asm-generic/ioctls.h` numbers it for x86, Arm and RISC-V, the
/// architectures ZFSBootMenu is built for.
const TIOCCONS: Opcode = 0x541D;

/// Has what programs write to `/dev/console` go to the terminal open as
/// `fd` instead of the kernel console; the kernel undoes it when that
/// terminal is hung up.
///
/// # Errors
///
/// When `fd` is not a terminal, the process may not (`EPERM`), or another
/// terminal has it already (`EBUSY`).
pub fn redirect_console(fd: impl AsFd) -> io::Result<()> {
    // SAFETY: `TIOCCONS` takes no argument and touches no memory of this
    // process.
    Ok(unsafe { ioctl(fd, NoArg::<TIOCCONS>::new()) }?)
}

/// The size of the bitmap of keys down that an input device reports: one
/// bit for each key code up to `KEY_MAX` (`0x2ff`).
pub const KEY_BITMAP: usize = 0x300 / 8;

/// `EVIOCGKEY(KEY_BITMAP)`: the keys down on an input device
/// (`linux/input.h`).
const EVIOCGKEY: Opcode = opcode::read::<[u8; KEY_BITMAP]>(b'E', 0x18);

/// `EVIOCSREP`: sets the delay and period of an input device's key repeat
/// (`linux/input.h`).
const EVIOCSREP: Opcode = opcode::write::<[u32; 2]>(b'E', 0x03);

/// Has the input device open as `fd` repeat a key held for `delay`
/// milliseconds every `period` milliseconds.
///
/// # Errors
///
/// When `fd` is not an input device, or one that repeats no keys.
pub fn set_key_repeat(fd: impl AsFd, delay: u32, period: u32) -> io::Result<()> {
    // SAFETY: `EVIOCSREP` reads two `unsigned int`s from the address it is
    // given, which `Setter` points at its own `[u32; 2]`.
    Ok(unsafe { ioctl(fd, Setter::<EVIOCSREP, [u32; 2]>::new([delay, period])) }?)
}

/// The key codes held down on the input device open as `fd`, as the kernel
/// knows them now: after events were dropped, what a reader of the device
/// resynchronises from.
///
/// # Errors
///
/// When `fd` is not an input device.
pub fn keys_down(fd: impl AsFd) -> io::Result<Vec<u16>> {
    // SAFETY: `EVIOCGKEY(KEY_BITMAP)` writes at most `KEY_BITMAP` bytes to
    // the address it is given, which `Getter` points at a buffer of that
    // size; any bytes are a valid `[u8; KEY_BITMAP]`.
    let mut bitmap = unsafe { ioctl(fd, Getter::<EVIOCGKEY, [u8; KEY_BITMAP]>::new()) }?;
    let down = codes_of(&bitmap);
    // Which keys are down tells a passphrase being typed.
    crate::wipe(&mut bitmap);
    Ok(down)
}

/// The key codes whose bits are set in `bitmap`, lowest first.
fn codes_of(bitmap: &[u8]) -> Vec<u16> {
    let mut codes = Vec::new();
    for (index, &byte) in bitmap.iter().enumerate() {
        for bit in 0..8_u16 {
            if byte >> bit & 1 == 1
                && let Ok(base) = u16::try_from(index * 8)
            {
                codes.push(base + bit);
            }
        }
    }
    codes
}

/// Runs a terminal `ioctl` whose argument is passed by value, as an
/// `unsigned long`: `KDSETMODE` or `KDSKBMODE`.
fn set_mode<const OPCODE: Opcode>(fd: impl AsFd, value: c_int) -> io::Result<()> {
    let value = usize::try_from(value).map_err(|_| io::Error::from_raw_os_error(EINVAL))?;
    // SAFETY: these requests take their argument by value; the kernel reads
    // nothing from this process's memory.
    Ok(unsafe { ioctl(fd, IntegerSetter::<OPCODE>::new_usize(value)) }?)
}

/// `EINVAL`, the same on every Linux architecture.
const EINVAL: i32 = 22;

/// Sets what the virtual terminal open as `fd` shows.
///
/// # Errors
///
/// When `fd` is not a virtual terminal (`ENOTTY`), or the process may not
/// change it.
pub fn set_display_mode(fd: impl AsFd, mode: DisplayMode) -> io::Result<()> {
    let value = match mode {
        DisplayMode::Text => 0,
        DisplayMode::Graphics => 1,
    };
    set_mode::<KDSETMODE>(fd, value)
}

/// The keyboard mode of the virtual terminal open as `fd`.
///
/// # Errors
///
/// When `fd` is not a virtual terminal.
pub fn keyboard_mode(fd: impl AsFd) -> io::Result<KeyboardMode> {
    // SAFETY: `KDGKBMODE` writes one `int` to the address it is given,
    // which `Getter` points at its own `c_int`.
    let mode = unsafe { ioctl(fd, Getter::<KDGKBMODE, c_int>::new()) }?;
    Ok(KeyboardMode(mode))
}

/// Sets the keyboard mode of the virtual terminal open as `fd`.
///
/// # Errors
///
/// When `fd` is not a virtual terminal, or the mode is not one.
pub fn set_keyboard_mode(fd: impl AsFd, mode: KeyboardMode) -> io::Result<()> {
    set_mode::<KDSKBMODE>(fd, mode.0)
}

/// A file to wait on: for data to read unless `read` is off, for room to
/// write when `write` is set, and always for an error or a hang-up.
#[derive(Clone, Copy, Debug)]
pub struct Watch<'fd> {
    /// The file.
    pub fd: BorrowedFd<'fd>,
    /// Whether to wait for data to read.
    pub read: bool,
    /// Whether to wait for room to write.
    pub write: bool,
}

impl<'fd> Watch<'fd> {
    /// Waits on `fd` for data to read.
    pub fn read(fd: BorrowedFd<'fd>) -> Watch<'fd> {
        Watch {
            fd,
            read: true,
            write: false,
        }
    }
}

/// What a file waited on is ready for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Ready {
    /// It has data to read.
    pub read: bool,
    /// It has room to write.
    pub write: bool,
    /// It has an error, was hung up or is not open: a read tells which.
    pub closed: bool,
}

impl Ready {
    /// Whether the file is ready for anything.
    pub fn is_ready(self) -> bool {
        self.read || self.write || self.closed
    }
}

/// Waits until one of `watches` is ready, for at most `timeout` (`None`:
/// without limit); returns what each is ready for, all `Ready::default()`
/// when the time ran out.
///
/// # Errors
///
/// When `poll` fails other than by a signal.
pub fn wait_ready(watches: &[Watch<'_>], timeout: Option<Duration>) -> io::Result<Vec<Ready>> {
    // A time-out too long for a `timespec` waits without limit.
    let timeout = timeout.and_then(|timeout| Timespec::try_from(timeout).ok());
    let mut entries: Vec<PollFd<'_>> = watches
        .iter()
        .map(|watch| {
            let mut events = PollFlags::empty();
            events.set(PollFlags::IN, watch.read);
            events.set(PollFlags::OUT, watch.write);
            PollFd::from_borrowed_fd(watch.fd, events)
        })
        .collect();
    let closed = PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL;
    loop {
        match rustix::event::poll(&mut entries, timeout.as_ref()) {
            Ok(_) => {
                return Ok(entries
                    .iter()
                    .map(|entry| {
                        let revents = entry.revents();
                        Ready {
                            read: revents.contains(PollFlags::IN),
                            write: revents.contains(PollFlags::OUT),
                            closed: revents.intersects(closed),
                        }
                    })
                    .collect());
            }
            Err(rustix::io::Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

/// Waits until one of `fds` has data to read, for at most `timeout`
/// (`None`: without limit); returns the index of the first that has, or
/// `None` when the time ran out.
///
/// # Errors
///
/// When `poll` fails other than by a signal.
pub fn first_readable(
    fds: &[BorrowedFd<'_>],
    timeout: Option<Duration>,
) -> io::Result<Option<usize>> {
    let watches: Vec<Watch<'_>> = fds.iter().map(|&fd| Watch::read(fd)).collect();
    let ready = wait_ready(&watches, timeout)?;
    Ok(ready.iter().position(|ready| ready.is_ready()))
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "zbm-sys is tested against the kernel itself: real programs, files and time"
)]
mod tests {
    use std::io::Write;
    use std::os::fd::AsFd as _;
    use std::os::unix::net::UnixStream;

    use super::*;

    #[test]
    fn modes_of_what_is_not_a_virtual_terminal_are_refused() {
        let null = std::fs::File::open("/dev/null").unwrap();
        let fd = null.as_fd();

        assert!(set_display_mode(fd, DisplayMode::Graphics).is_err());
        assert!(set_display_mode(fd, DisplayMode::Text).is_err());
        assert!(keyboard_mode(fd).is_err());
        assert!(set_keyboard_mode(fd, KeyboardMode::OFF).is_err());
        let negative = set_keyboard_mode(fd, KeyboardMode(-1)).unwrap_err();
        assert_eq!(negative.raw_os_error(), Some(EINVAL));
        assert!(keys_down(fd).is_err());
        // Not a terminal: `/dev/console` is left where it is.
        assert!(redirect_console(fd).is_err());
    }

    #[test]
    fn the_keys_down_are_read_from_the_bitmap() {
        let mut bitmap = [0_u8; KEY_BITMAP];
        bitmap[0] = 0b10;
        bitmap[3] = 0b100;
        bitmap[KEY_BITMAP - 1] = 0x80;

        assert_eq!(codes_of(&bitmap), [1, 26, 0x2ff]);
        // As `linux/input.h` numbers them on x86 and Arm.
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "x86"))]
        {
            assert_eq!(EVIOCGKEY, 0x8060_4518);
            assert_eq!(EVIOCSREP, 0x4008_4503);
            assert_eq!(TIOCCONS, 0x541D);
        }
    }

    #[test]
    fn files_are_ready_to_read_to_write_or_closed() {
        let (mut a, a_end) = UnixStream::pair().unwrap();
        let (b, b_end) = UnixStream::pair().unwrap();
        let watches = [
            Watch::read(a_end.as_fd()),
            Watch {
                fd: b_end.as_fd(),
                read: true,
                write: true,
            },
        ];

        let ready = wait_ready(&watches, Some(Duration::ZERO)).unwrap();
        assert_eq!(ready[0], Ready::default());
        assert!(ready[1].write && !ready[1].read && ready[1].is_ready());
        a.write_all(b"x").unwrap();
        drop(b);
        let ready = wait_ready(&watches, None).unwrap();
        assert!(ready[0].read && !ready[0].write);
        assert!(ready[1].closed);
        assert!(!Ready::default().is_ready());
    }

    #[test]
    fn the_first_input_with_data_is_found() {
        let (mut a, a_end) = UnixStream::pair().unwrap();
        let (mut b, b_end) = UnixStream::pair().unwrap();
        let fds = [a_end.as_fd(), b_end.as_fd()];

        assert_eq!(
            first_readable(&fds, Some(Duration::from_millis(10))).unwrap(),
            None
        );
        b.write_all(b"x").unwrap();
        assert_eq!(first_readable(&fds, None).unwrap(), Some(1));
        a.write_all(b"y").unwrap();
        assert_eq!(first_readable(&fds, Some(Duration::ZERO)).unwrap(), Some(0));
    }
}
