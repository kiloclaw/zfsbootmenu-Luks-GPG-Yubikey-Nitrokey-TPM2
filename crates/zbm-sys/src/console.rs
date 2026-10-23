//! What a program that draws on the display itself needs from the kernel:
//! the virtual terminal's display and keyboard modes, which it takes over
//! while it draws and gives back before it ends (`zbm-display`), and a wait
//! on several input devices at once.

use std::ffi::{c_int, c_ulong};
use std::io;
use std::time::Duration;

use super::{POLLIN, PollFd, poll};

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

/// Runs an `ioctl` of the virtual terminal whose argument is an `int`
/// passed by value: `KDSETMODE` or `KDSKBMODE`.
fn set_mode(fd: i32, request: c_ulong, value: c_int) -> io::Result<()> {
    // SAFETY: these requests take their argument by value; the kernel reads
    // nothing from this process's memory.
    checked(unsafe { ioctl(fd, request, value) })
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

/// Waits until one of `fds` has data to read, for at most `timeout`
/// (`None`: without limit); returns the index of the first that has, or
/// `None` when the time ran out.
///
/// # Errors
///
/// When `poll` fails other than by a signal.
pub fn first_readable(fds: &[i32], timeout: Option<Duration>) -> io::Result<Option<usize>> {
    let timeout = timeout.map_or(-1, |timeout| {
        let millis = timeout.as_nanos().div_ceil(1_000_000);
        c_int::try_from(millis).unwrap_or(c_int::MAX)
    });
    let mut entries: Vec<PollFd> = fds
        .iter()
        .map(|&fd| PollFd {
            fd,
            events: POLLIN,
            revents: 0,
        })
        .collect();
    let count = c_ulong::try_from(entries.len()).unwrap_or(c_ulong::MAX);
    loop {
        // SAFETY: `entries` holds `count` valid `struct pollfd`s and lives
        // across the call.
        let ready = unsafe { poll(entries.as_mut_ptr(), count, timeout) };
        if ready >= 0 {
            return Ok(entries.iter().position(|entry| entry.revents != 0));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
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
