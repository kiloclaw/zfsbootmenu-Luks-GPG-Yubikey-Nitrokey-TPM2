//! HID feature reports through `hidraw` (`linux/hidraw.h`): what a
//! YubiKey's challenge-response speaks (ROADMAP.md SB11). The requests are
//! `ioctl`s that `rustix` has no wrapper for: they go through its typed
//! `ioctl`, with the opcodes built for the target by `rustix::ioctl::opcode`.

use std::io;
use std::os::fd::AsFd;

use rustix::ioctl::{Opcode, Updater, ioctl, opcode};

/// The length of a feature report of the YubiKey's OTP interface.
pub const REPORT: usize = 8;

/// A feature report with its number before it, as the requests take it.
type Numbered = [u8; REPORT + 1];

/// `HIDIOCSFEATURE(9)`: sends a feature report.
const HIDIOCSFEATURE: Opcode = opcode::read_write::<Numbered>(b'H', 0x06);

/// `HIDIOCGFEATURE(9)`: reads a feature report.
const HIDIOCGFEATURE: Opcode = opcode::read_write::<Numbered>(b'H', 0x07);

/// Sends `report`, as feature report 0, to the `hidraw` device open as `fd`.
///
/// # Errors
///
/// When `fd` is no `hidraw` device, or the device refuses the report.
pub fn set_feature(fd: impl AsFd, report: &[u8; REPORT]) -> io::Result<()> {
    let mut numbered: Numbered = [0; REPORT + 1];
    numbered[1..].copy_from_slice(report);
    // SAFETY: `HIDIOCSFEATURE(9)` reads nine bytes from the address it is
    // given, the report number and the report, and writes nothing beyond
    // them; `Updater` points at `numbered`, which is nine bytes.
    let sent = unsafe { ioctl(fd, Updater::<HIDIOCSFEATURE, Numbered>::new(&mut numbered)) };
    // The report may hold part of a challenge.
    crate::wipe(&mut numbered);
    Ok(sent?)
}

/// Reads feature report 0 from the `hidraw` device open as `fd` into
/// `report`.
///
/// # Errors
///
/// When `fd` is no `hidraw` device, or the device gives no such report.
pub fn get_feature(fd: impl AsFd, report: &mut [u8; REPORT]) -> io::Result<()> {
    let mut numbered: Numbered = [0; REPORT + 1];
    // SAFETY: `HIDIOCGFEATURE(9)` reads the report number from the first
    // byte at the address it is given and writes at most nine bytes there;
    // `Updater` points at `numbered`, which is nine bytes, any of which are
    // a valid `u8`.
    let read = unsafe { ioctl(fd, Updater::<HIDIOCGFEATURE, Numbered>::new(&mut numbered)) };
    // The kernel leaves the report number first.
    report.copy_from_slice(&numbered[1..]);
    crate::wipe(&mut numbered);
    Ok(read?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "a test of the requests on a real file"
    )]
    fn a_file_that_is_no_hid_device_refuses_the_reports() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let mut report = [0xff; REPORT];
        assert!(set_feature(&file, &[0; REPORT]).is_err());
        assert!(get_feature(&file, &mut report).is_err());
    }

    #[test]
    fn the_opcodes_are_those_of_the_header() {
        // `_IOC(_IOC_WRITE|_IOC_READ, 'H', 0x06, 9)` and `0x07` on x86,
        // Arm and RISC-V.
        assert_eq!(HIDIOCSFEATURE, 0xC009_4806);
        assert_eq!(HIDIOCGFEATURE, 0xC009_4807);
    }
}
