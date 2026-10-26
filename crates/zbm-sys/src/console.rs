//! What a program that draws on the display itself needs from the kernel:
//! the virtual terminal's display and keyboard modes, which it takes over
//! while it draws and gives back before it ends (`zbm-display`), and a wait
//! on several input devices at once.

use std::ffi::{c_int, c_ulong};
use std::io;
use std::time::Duration;

use super::{POLLIN, PollFd, poll};

/// `POLLOUT`: room to write.
const POLLOUT: i16 = 4;

/// `POLLERR`, `POLLHUP` and `POLLNVAL`: the file has an error, was hung up
/// or is not open.
const POLL_CLOSED: i16 = 0x08 | 0x10 | 0x20;

unsafe extern "C" {
    // `int ioctl(int fd, unsigned long request, ...)` in glibc. musl takes an
    // `int` request, which arrives in the same register for these small
    // values.
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

/// `KDSETMODE`: text or graphics on a virtual terminal (`linux/kd.h`).
const KDSETMODE: c_ulong = 0x4B3A;

/// `KDGKBMODE`: the keyboard mode of a virtual terminal.
const KDGKBMODE: c_ulong = 0x4B44;

/// `KDSKBMODE`: sets the keyboard mode of a virtual terminal.
const KDSKBMODE: c_ulong = 0x4B45;

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

/// The result of an `ioctl`: an error from `errno` when it is negative.
fn checked(result: c_int) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Runs an `ioctl` of a terminal whose argument, if it has one, is passed
/// by value: `KDSETMODE`, `KDSKBMODE` or `TIOCCONS`. The kernel reads it as
/// an `unsigned long`, so it is passed as one: an `int` in a variadic call
/// leaves the upper half of the register undefined.
fn set_mode(fd: i32, request: c_ulong, value: c_int) -> io::Result<()> {
    let value = c_ulong::try_from(value).map_err(|_| io::Error::from_raw_os_error(EINVAL))?;
    // SAFETY: these requests take their argument by value, or none; the
    // kernel reads nothing from this process's memory.
    checked(unsafe { ioctl(fd, request, value) })
}

/// `TIOCCONS`: writes to `/dev/console` go to this terminal
/// (`asm-generic/ioctls.h`, which x86 and Arm use).
const TIOCCONS: c_ulong = 0x541D;

/// Has what programs write to `/dev/console` go to the terminal open as
/// `fd` instead of the kernel console; the kernel undoes it when that
/// terminal is hung up.
///
/// # Errors
///
/// When `fd` is not a terminal, the process may not (`EPERM`), or another
/// terminal has it already (`EBUSY`).
pub fn redirect_console(fd: i32) -> io::Result<()> {
    set_mode(fd, TIOCCONS, 0)
}

/// `EINVAL`, the same on every Linux architecture.
const EINVAL: i32 = 22;

/// The size of the bitmap of keys down that an input device reports: one
/// bit for each key code up to `KEY_MAX` (`0x2ff`).
pub const KEY_BITMAP: usize = 0x300 / 8;

/// `EVIOCGKEY(KEY_BITMAP)`: the keys down on an input device
/// (`linux/input.h`), an `_IOR('E', 0x18, len)`.
const EVIOCGKEY: c_ulong = 2 << 30 | (KEY_BITMAP as c_ulong) << 16 | 0x45 << 8 | 0x18;

/// `EVIOCSREP`: sets the delay and period of an input device's key repeat
/// (`linux/input.h`), an `_IOW('E', 0x03, unsigned int[2])`.
const EVIOCSREP: c_ulong = 1 << 30 | 8 << 16 | 0x45 << 8 | 0x03;

/// Has the input device open as `fd` repeat a key held for `delay`
/// milliseconds every `period` milliseconds.
///
/// # Errors
///
/// When `fd` is not an input device, or one that repeats no keys.
pub fn set_key_repeat(fd: i32, delay: u32, period: u32) -> io::Result<()> {
    let values = [delay, period];
    // SAFETY: `EVIOCSREP` reads two `unsigned int`s from the address it is
    // given, which is `values`, alive for the call.
    checked(unsafe { ioctl(fd, EVIOCSREP, values.as_ptr()) }).map(drop)
}

/// The key codes held down on the input device open as `fd`, as the kernel
/// knows them now: after events were dropped, what a reader of the device
/// resynchronises from.
///
/// # Errors
///
/// When `fd` is not an input device.
pub fn keys_down(fd: i32) -> io::Result<Vec<u16>> {
    let mut bitmap = [0_u8; KEY_BITMAP];
    // SAFETY: `EVIOCGKEY(KEY_BITMAP)` writes at most `KEY_BITMAP` bytes to
    // the address it is given, which is `bitmap`, alive for the call.
    checked(unsafe { ioctl(fd, EVIOCGKEY, bitmap.as_mut_ptr()) })?;
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

/// Sets what the virtual terminal open as `fd` shows.
///
/// # Errors
///
/// When `fd` is not a virtual terminal (`ENOTTY`), or the process may not
/// change it.
pub fn set_display_mode(fd: i32, mode: DisplayMode) -> io::Result<()> {
    let value = match mode {
        DisplayMode::Text => 0,
        DisplayMode::Graphics => 1,
    };
    set_mode(fd, KDSETMODE, value)
}

/// The keyboard mode of the virtual terminal open as `fd`.
///
/// # Errors
///
/// When `fd` is not a virtual terminal.
pub fn keyboard_mode(fd: i32) -> io::Result<KeyboardMode> {
    let mut mode: c_int = 0;
    // SAFETY: `KDGKBMODE` writes one `int` to the address it is given, which
    // is `mode`, alive for the call.
    checked(unsafe { ioctl(fd, KDGKBMODE, &raw mut mode) })?;
    Ok(KeyboardMode(mode))
}

/// Sets the keyboard mode of the virtual terminal open as `fd`.
///
/// # Errors
///
/// When `fd` is not a virtual terminal, or the mode is not one.
pub fn set_keyboard_mode(fd: i32, mode: KeyboardMode) -> io::Result<()> {
    set_mode(fd, KDSKBMODE, mode.0)
}

/// A file to wait on: for data to read unless `read` is off, for room to
/// write when `write` is set, and always for an error or a hang-up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Watch {
    /// The file.
    pub fd: i32,
    /// Whether to wait for data to read.
    pub read: bool,
    /// Whether to wait for room to write.
    pub write: bool,
}

impl Watch {
    /// Waits on `fd` for data to read.
    pub fn read(fd: i32) -> Watch {
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
pub fn wait_ready(watches: &[Watch], timeout: Option<Duration>) -> io::Result<Vec<Ready>> {
    let timeout = timeout.map_or(-1, |timeout| {
        let millis = timeout.as_nanos().div_ceil(1_000_000);
        c_int::try_from(millis).unwrap_or(c_int::MAX)
    });
    let mut entries: Vec<PollFd> = watches
        .iter()
        .map(|watch| PollFd {
            fd: watch.fd,
            events: if watch.read { POLLIN } else { 0 } | if watch.write { POLLOUT } else { 0 },
            revents: 0,
        })
        .collect();
    let count = c_ulong::try_from(entries.len()).unwrap_or(c_ulong::MAX);
    loop {
        // SAFETY: `entries` holds `count` valid `struct pollfd`s and lives
        // across the call.
        let ready = unsafe { poll(entries.as_mut_ptr(), count, timeout) };
        if ready >= 0 {
            return Ok(entries
                .iter()
                .map(|entry| Ready {
                    read: entry.revents & POLLIN != 0,
                    write: entry.revents & POLLOUT != 0,
                    closed: entry.revents & POLL_CLOSED != 0,
                })
                .collect());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
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
pub fn first_readable(fds: &[i32], timeout: Option<Duration>) -> io::Result<Option<usize>> {
    let watches: Vec<Watch> = fds.iter().map(|&fd| Watch::read(fd)).collect();
    let ready = wait_ready(&watches, timeout)?;
    Ok(ready.iter().position(|ready| ready.is_ready()))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    use super::*;

    #[test]
    fn modes_of_what_is_not_a_virtual_terminal_are_refused() {
        let null = std::fs::File::open("/dev/null").unwrap();
        let fd = null.as_raw_fd();

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
        assert_eq!(EVIOCGKEY, 0x8060_4518);
        assert_eq!(EVIOCSREP, 0x4008_4503);
    }

    #[test]
    fn files_are_ready_to_read_to_write_or_closed() {
        let (mut a, a_end) = UnixStream::pair().unwrap();
        let (b, b_end) = UnixStream::pair().unwrap();
        let watches = [
            Watch::read(a_end.as_raw_fd()),
            Watch {
                fd: b_end.as_raw_fd(),
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
        let fds = [a_end.as_raw_fd(), b_end.as_raw_fd()];

        assert_eq!(
            first_readable(&fds, Some(Duration::from_millis(10))).unwrap(),
            None
        );
        b.write_all(b"x").unwrap();
        assert_eq!(first_readable(&fds, None).unwrap(), Some(1));
        a.write_all(b"y").unwrap();
        assert_eq!(first_readable(&fds, Some(Duration::ZERO)).unwrap(), Some(0));
        assert!(
            first_readable(&[-5], Some(Duration::ZERO))
                .unwrap()
                .is_none()
        );
    }
}
