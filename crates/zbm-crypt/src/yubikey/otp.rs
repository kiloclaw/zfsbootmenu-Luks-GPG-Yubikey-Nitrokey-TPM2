//! A YubiKey's HMAC-SHA1 challenge-response and serial number over its OTP
//! interface (ROADMAP.md SB11), as `ykchalresp` and `ykinfo` ask for them,
//! without them: their library, `ykpers`, is no longer maintained.
//!
//! The protocol is the one `ykpers`' `ykcore` speaks. A command is a frame
//! of 70 bytes (64 bytes of payload, the command, the CRC-16 of the
//! payload, three bytes of filler), sent as feature reports of 7 bytes and
//! a sequence number with the write flag; the key clears the flag when it
//! took a part, and parts that are all zeros are left out but the first and
//! the last. The key then sets the pending flag and gives its response in
//! parts of 7 bytes, numbered from 0, with its own CRC, until the number
//! goes back to 0; a dummy report ends reading. While it waits for a touch,
//! the key sets the timeout flag.
//!
//! The devices are found in `/sys/class/hidraw`: Yubico's vendor ID, and a
//! keyboard's report descriptor, the OTP interface (the FIDO interface has
//! a descriptor of its own).

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use zbm_platform::hid::{FeatureReports, HidDevices, REPORT, Report};
use zbm_platform::{Clock, Files, Secret};

/// Yubico's USB vendor ID.
pub const VENDOR: u32 = 0x1050;

/// Where the kernel lists its `hidraw` devices.
pub const HIDRAW_CLASS: &str = "/sys/class/hidraw";

/// The longest challenge: what `ykchalresp` reads of one, and sends.
pub const CHALLENGE: usize = 64;

/// The command of HMAC-SHA1 challenge-response of slot 1; slot 2's is
/// `0x38`.
const CHAL_HMAC1: u8 = 0x30;

/// The command of slot 2.
const CHAL_HMAC2: u8 = 0x38;

/// The command that reads the serial number.
const DEVICE_SERIAL: u8 = 0x10;

/// Set by the host on a part it sends, cleared by the key once it took it.
const WRITE_FLAG: u8 = 0x80;

/// Set by the key while it has a response to give.
const PENDING_FLAG: u8 = 0x40;

/// Set by the key while it waits for a touch.
const TOUCH_FLAG: u8 = 0x20;

/// The sequence number of a part of a response.
const SEQUENCE: u8 = 0x1f;

/// The status byte of a report that ends reading, an invalid sequence.
const DUMMY: u8 = 0x8f;

/// The CRC-16 of data followed by its own CRC, complemented, as the key
/// appends it.
const CRC_RESIDUE: u16 = 0xf0b8;

/// The bytes of a frame.
const FRAME: usize = 70;

/// The data of a report, before its status byte.
const PART: usize = REPORT - 1;

/// The longest wait for the key to take a part, as `ykcore` waits.
const WRITE_WAIT: Duration = Duration::from_millis(1150);

/// The longest wait for a response to begin.
const RESPONSE_WAIT: Duration = Duration::from_secs(1);

/// The longer wait once the key says it waits for a touch: it gives up
/// itself after 15 seconds.
const TOUCH_WAIT: Duration = Duration::from_secs(30);

/// The longest pause between two looks at the key.
const LONGEST_PAUSE: Duration = Duration::from_millis(500);

/// Why the YubiKey gave no answer.
#[derive(Debug, thiserror::Error)]
pub enum OtpError {
    /// The device could not be opened, read or written.
    #[error("the YubiKey could not be reached: {0}")]
    Io(#[from] io::Error),
    /// It did not take a part of the command in time.
    #[error("the YubiKey is busy")]
    Busy,
    /// It gave no response: the slot is not set up for challenge-response,
    /// or the key gives no serial number.
    #[error("the YubiKey gave no response")]
    NoResponse,
    /// It waited for a touch, and nobody touched it.
    #[error("the YubiKey was not touched")]
    NotTouched,
    /// Its response does not match its checksum, or is of another length.
    #[error("the YubiKey's response is damaged")]
    Damaged,
    /// There is no slot of that number.
    #[error("the YubiKey has no slot {0}")]
    NoSlot(u8),
}

/// The CRC-16 of `bytes` (ISO 13239, without the final complement), as the
/// key and `ykcore` compute it.
fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0xffff, |crc, &byte| {
        (0..8).fold(crc ^ u16::from(byte), |crc, _| {
            if crc & 1 == 1 {
                (crc >> 1) ^ 0x8408
            } else {
                crc >> 1
            }
        })
    })
}

/// The YubiKeys' OTP interfaces among the `hidraw` devices, by their
/// device nodes, in order.
pub fn devices(files: &dyn Files) -> Vec<PathBuf> {
    let class = Path::new(HIDRAW_CLASS);
    let mut found: Vec<PathBuf> = files
        .read_dir(class)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| {
            let device = class.join(name).join("device");
            let uevent = files
                .read_to_string(&device.join("uevent"))
                .unwrap_or_default();
            let descriptor = files
                .read(&device.join("report_descriptor"))
                .unwrap_or_default();
            is_yubico(&uevent) && is_keyboard(&descriptor)
        })
        .map(|name| Path::new("/dev").join(name))
        .collect();
    found.sort();
    found
}

/// Whether the kernel lists `hidraw` devices at all, so that the
/// [`devices`] it finds none of are not there.
pub fn has_hidraw(files: &dyn Files) -> bool {
    files.is_dir(Path::new(HIDRAW_CLASS))
}

/// Whether the `uevent` text of a HID device names Yubico as its vendor
/// (`HID_ID=0003:00001050:00000407`: bus, vendor, product).
fn is_yubico(uevent: &str) -> bool {
    uevent.lines().any(|line| {
        line.strip_prefix("HID_ID=")
            .and_then(|id| id.split(':').nth(1))
            .and_then(|vendor| u32::from_str_radix(vendor, 16).ok())
            == Some(VENDOR)
    })
}

/// Whether a report descriptor is a keyboard's: the generic desktop usage
/// page (`05 01`), then the keyboard usage (`09 06`).
fn is_keyboard(descriptor: &[u8]) -> bool {
    descriptor.starts_with(&[0x05, 0x01, 0x09, 0x06])
}

/// The protocol, on the devices of `hid`, timed by `clock`.
#[derive(Clone, Copy)]
pub struct Otp<'a> {
    hid: &'a dyn HidDevices,
    clock: &'a dyn Clock,
}

impl<'a> Otp<'a> {
    /// The protocol on `hid`'s devices.
    pub fn new(hid: &'a dyn HidDevices, clock: &'a dyn Clock) -> Otp<'a> {
        Otp { hid, clock }
    }

    /// The HMAC-SHA1 response of slot `slot` (1 or 2) of the YubiKey at
    /// `device` to `challenge`, as `ykchalresp -<slot> -i-` prints it: in
    /// lower-case hexadecimal. As `ykchalresp`, it sends the first
    /// [`CHALLENGE`] bytes of the challenge.
    ///
    /// # Errors
    ///
    /// Why the key gave none.
    pub fn response(
        &self,
        device: &Path,
        slot: u8,
        challenge: &Secret,
    ) -> Result<Secret, OtpError> {
        let command = match slot {
            1 => CHAL_HMAC1,
            2 => CHAL_HMAC2,
            _ => return Err(OtpError::NoSlot(slot)),
        };
        let mut key = self.hid.open(device)?;
        let sent = challenge
            .expose()
            .get(..CHALLENGE)
            .unwrap_or(challenge.expose());
        let response = self.command(key.as_mut(), command, sent, 20)?;
        let mut text = Secret::new();
        for byte in response.expose() {
            for nibble in [byte >> 4, byte & 0x0f] {
                text.push(if nibble < 10 {
                    b'0' + nibble
                } else {
                    b'a' + nibble - 10
                });
            }
        }
        Ok(text)
    }

    /// The serial number of the YubiKey at `device`, as `ykinfo -s` prints
    /// it.
    ///
    /// # Errors
    ///
    /// Why the key gave none: [`OtpError::NoResponse`] when it shows no
    /// serial number.
    pub fn serial(&self, device: &Path) -> Result<u32, OtpError> {
        let mut key = self.hid.open(device)?;
        let response = self.command(key.as_mut(), DEVICE_SERIAL, &[], 4)?;
        let bytes: [u8; 4] = response
            .expose()
            .try_into()
            .map_err(|_| OtpError::Damaged)?;
        Ok(u32::from_be_bytes(bytes))
    }

    /// Sends `command` with `payload`, and reads its response of
    /// `expected` bytes; a dummy report ends the reading on every path.
    fn command(
        &self,
        key: &mut dyn FeatureReports,
        command: u8,
        payload: &[u8],
        expected: usize,
    ) -> Result<Secret, OtpError> {
        self.write(key, command, payload)?;
        let response = self.read(key, expected);
        let dummy = Report([0, 0, 0, 0, 0, 0, 0, DUMMY]);
        let ended = key.set(&dummy);
        let response = response?;
        ended?;
        Ok(response)
    }

    /// Sends the frame of `command` with `payload`, part by part.
    fn write(
        &self,
        key: &mut dyn FeatureReports,
        command: u8,
        payload: &[u8],
    ) -> Result<(), OtpError> {
        let mut frame = Secret::copy_from(payload);
        while frame.len() < CHALLENGE {
            frame.push(0);
        }
        let crc = crc16(frame.expose());
        frame.push(command);
        frame.push_bytes(&crc.to_le_bytes());
        frame.push_bytes(&[0; FRAME - CHALLENGE - 3]);
        let parts = frame.expose().chunks(PART);
        let last = FRAME / PART - 1;
        for (sequence, part) in parts.enumerate() {
            if part.iter().all(|&byte| byte == 0) && sequence != 0 && sequence != last {
                continue;
            }
            let mut report = Report::default();
            report.0[..PART].copy_from_slice(part);
            report.0[PART] = WRITE_FLAG | u8::try_from(sequence).unwrap_or(SEQUENCE);
            self.wait(key, WRITE_WAIT, |status| status & WRITE_FLAG == 0)
                .map_err(|error| match error {
                    OtpError::NoResponse => OtpError::Busy,
                    error => error,
                })?;
            key.set(&report)?;
        }
        Ok(())
    }

    /// Reads a response of `expected` bytes, and checks its CRC.
    fn read(&self, key: &mut dyn FeatureReports, expected: usize) -> Result<Secret, OtpError> {
        let first = self.wait(key, RESPONSE_WAIT, |status| status & PENDING_FLAG != 0)?;
        let mut response = Secret::copy_from(&first.0[..PART]);
        // With its CRC, in whole parts.
        let length = (expected + 2).div_ceil(PART) * PART;
        loop {
            let mut report = Report::default();
            key.get(&mut report)?;
            let status = report.0[PART];
            if status & PENDING_FLAG == 0 {
                return Err(OtpError::Damaged);
            }
            if status & SEQUENCE == 0 {
                break;
            }
            if response.len() >= length {
                return Err(OtpError::Damaged);
            }
            response.push_bytes(&report.0[..PART]);
        }
        let checked = response.expose().get(..expected + 2).unwrap_or_default();
        if response.len() != length || crc16(checked) != CRC_RESIDUE {
            return Err(OtpError::Damaged);
        }
        while response.len() > expected {
            response.pop();
        }
        Ok(response)
    }

    /// Reads the key's status until `done` holds for it, for `longest` at
    /// most, or [`TOUCH_WAIT`] once the key waits for a touch; the report
    /// it held then.
    fn wait(
        &self,
        key: &mut dyn FeatureReports,
        longest: Duration,
        done: impl Fn(u8) -> bool,
    ) -> Result<Report, OtpError> {
        let start = self.clock.now();
        let mut limit = longest;
        let mut touch = false;
        let mut pause = Duration::from_millis(1);
        loop {
            let mut report = Report::default();
            key.get(&mut report)?;
            let status = report.0[PART];
            if done(status) {
                return Ok(report);
            }
            if status & TOUCH_FLAG != 0 {
                if !touch {
                    touch = true;
                    limit += TOUCH_WAIT;
                }
            } else if touch {
                // The key gave up waiting for the touch.
                return Err(OtpError::NotTouched);
            }
            if self.clock.now().saturating_sub(start) >= limit {
                return Err(if touch {
                    OtpError::NotTouched
                } else {
                    OtpError::NoResponse
                });
            }
            self.clock.sleep(pause);
            pause = (pause * 2).min(LONGEST_PAUSE);
        }
    }
}

#[cfg(test)]
mod tests;
