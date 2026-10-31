use std::sync::Mutex;

use pretty_assertions::assert_eq;
use sha1::{Digest as _, Sha1};
use zbm_testkit::fakes::{FakeHid, FakeSystem, FakeYubiKey, START};

use super::*;

/// HMAC-SHA1 of `message` with `key` (RFC 2104), as a YubiKey's slot
/// computes it, for a key of at most 64 bytes.
fn hmac_sha1(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut block = [0_u8; 64];
    block[..key.len()].copy_from_slice(key);
    let pad = |byte: u8| block.iter().map(|k| k ^ byte).collect::<Vec<u8>>();
    let inner = Sha1::new()
        .chain_update(pad(0x36))
        .chain_update(message)
        .finalize();
    Sha1::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .to_vec()
}

/// A key whose two slots hold the HMAC key of RFC 2202's first test case.
fn key() -> FakeYubiKey {
    FakeYubiKey::new(|_, challenge| Some(hmac_sha1(&[0x0b; 20], challenge)))
}

/// The fake system with `key` plugged in as `hidraw1`.
fn plugged(key: FakeYubiKey) -> FakeSystem {
    let fake = FakeSystem::new();
    fake.plug_yubikey("hidraw1", key);
    fake
}

const DEVICE: &str = "/dev/hidraw1";

#[test]
fn the_checksum_is_iso_13239_s() {
    // CRC-16/X-25's check value 0x906e, without its final complement.
    assert_eq!(crc16(b"123456789"), 0x6f91);
    let mut data = b"response".to_vec();
    data.extend_from_slice(&(!crc16(b"response")).to_le_bytes());
    assert_eq!(crc16(&data), CRC_RESIDUE);
}

#[test]
fn the_response_is_the_slot_s_hmac_in_hexadecimal() {
    let fake = plugged(key());
    let otp = Otp::new(&fake.hid, &fake.clock);

    let response = otp.response(Path::new(DEVICE), 2, &Secret::copy_from(b"Hi There"));

    // RFC 2202, test case 1.
    let expected = "b617318655057264e28bc0b6fb378c8ef146be00";
    assert_eq!(response.unwrap().expose(), expected.as_bytes());
    assert_eq!(fake.hid.challenges(DEVICE), [(2, b"Hi There".to_vec())]);
    assert!(fake.hid.is_idle(DEVICE));
    let slot1 = otp.response(Path::new(DEVICE), 1, &Secret::copy_from(b"x"));
    assert!(slot1.is_ok());
    assert_eq!(fake.hid.challenges(DEVICE)[1].0, 1);
}

#[test]
fn as_ykchalresp_only_the_first_64_bytes_are_sent() {
    let fake = plugged(key());
    let long: Vec<u8> = (0..80).collect();

    let response = Otp::new(&fake.hid, &fake.clock)
        .response(Path::new(DEVICE), 2, &Secret::copy_from(&long))
        .unwrap();

    // The key takes the trailing run of the last byte sent as padding
    // (`HMAC_LT64`): of the 64 bytes, the 64th.
    let sent = &long[..63];
    assert_eq!(fake.hid.challenges(DEVICE), [(2, sent.to_vec())]);
    let expected = crate::measure::hex(&hmac_sha1(&[0x0b; 20], sent));
    assert_eq!(response.expose(), expected.as_bytes());
}

#[test]
fn the_serial_number_is_read_big_endian() {
    let fake = plugged(key().with_serial(12_345_678));
    let otp = Otp::new(&fake.hid, &fake.clock);
    assert_eq!(otp.serial(Path::new(DEVICE)).unwrap(), 12_345_678);
    assert!(fake.hid.is_idle(DEVICE));

    let hidden = plugged(key());
    let otp = Otp::new(&hidden.hid, &hidden.clock);
    assert!(matches!(
        otp.serial(Path::new(DEVICE)),
        Err(OtpError::NoResponse)
    ));
    assert!(hidden.hid.is_idle(DEVICE));
}

#[test]
fn a_touch_is_waited_for_and_one_that_does_not_come_is_said_so() {
    let fake = plugged(key().waiting_for_touch(20));
    let otp = Otp::new(&fake.hid, &fake.clock);
    assert!(
        otp.response(Path::new(DEVICE), 2, &Secret::copy_from(b"c"))
            .is_ok()
    );
    // The pauses grow to half a second.
    assert!(fake.clock.sleeps().contains(&LONGEST_PAUSE));

    let untouched = plugged(key().waiting_for_touch(usize::MAX));
    let otp = Otp::new(&untouched.hid, &untouched.clock);
    let start = untouched.clock.now();
    let error = otp.response(Path::new(DEVICE), 2, &Secret::copy_from(b"c"));
    assert!(matches!(error, Err(OtpError::NotTouched)), "{error:?}");
    assert!(untouched.clock.now().saturating_sub(start) >= TOUCH_WAIT);
    assert!(untouched.hid.is_idle(DEVICE));
}

#[test]
fn a_busy_key_is_waited_for_until_it_is_too_busy() {
    let fake = plugged(key().slow(3));
    let otp = Otp::new(&fake.hid, &fake.clock);
    assert!(
        otp.response(Path::new(DEVICE), 1, &Secret::copy_from(b"c"))
            .is_ok()
    );

    let stuck = plugged(key().slow(usize::MAX));
    let otp = Otp::new(&stuck.hid, &stuck.clock);
    let error = otp.response(Path::new(DEVICE), 1, &Secret::copy_from(b"c"));
    assert!(matches!(error, Err(OtpError::Busy)), "{error:?}");
}

#[test]
fn a_slot_without_challenge_response_a_missing_slot_and_a_missing_key_give_nothing() {
    let fake = plugged(FakeYubiKey::new(|_, _| None));
    let otp = Otp::new(&fake.hid, &fake.clock);
    let start = fake.clock.now();
    let error = otp.response(Path::new(DEVICE), 2, &Secret::copy_from(b"c"));
    assert!(matches!(error, Err(OtpError::NoResponse)), "{error:?}");
    assert!(fake.clock.now().saturating_sub(start) < Duration::from_secs(2));
    assert!(matches!(
        otp.response(Path::new(DEVICE), 3, &Secret::copy_from(b"c")),
        Err(OtpError::NoSlot(3))
    ));
    let missing = otp.response(Path::new("/dev/hidraw9"), 2, &Secret::copy_from(b"c"));
    assert!(matches!(missing, Err(OtpError::Io(_))));
    let gone = plugged(key().gone());
    let otp = Otp::new(&gone.hid, &gone.clock);
    let error = otp.response(Path::new(DEVICE), 2, &Secret::copy_from(b"c"));
    assert_eq!(
        error.unwrap_err().to_string(),
        "the YubiKey could not be reached: No such device (os error 19)"
    );
}

/// A device that gives the reports of a script, in order, then idle ones,
/// and takes any.
struct Scripted(Mutex<Vec<[u8; 8]>>);

impl HidDevices for Scripted {
    fn open(&self, _path: &Path) -> io::Result<Box<dyn FeatureReports>> {
        let reports = std::mem::take(&mut *self.0.lock().unwrap());
        Ok(Box::new(Script(reports.into_iter().rev().collect())))
    }
}

/// The reports left, last first.
struct Script(Vec<[u8; 8]>);

impl FeatureReports for Script {
    fn get(&mut self, report: &mut Report) -> io::Result<()> {
        report.0 = self.0.pop().unwrap_or_default();
        Ok(())
    }

    fn set(&mut self, _report: &Report) -> io::Result<()> {
        Ok(())
    }
}

/// The serial number read from a device that gives `responses` after
/// taking the command's two parts.
fn serial_from(responses: &[[u8; 8]]) -> Result<u32, OtpError> {
    let mut script = vec![[0; 8], [0; 8]];
    script.extend_from_slice(responses);
    let fake = FakeSystem::new();
    let hid = Scripted(Mutex::new(script));
    Otp::new(&hid, &fake.clock).serial(Path::new(DEVICE))
}

#[test]
fn a_damaged_or_broken_off_response_is_refused() {
    let serial = 7_u32.to_be_bytes();
    let check = (!crc16(&serial)).to_le_bytes();
    let part = |status: u8, crc: [u8; 2]| {
        [
            serial[0], serial[1], serial[2], serial[3], crc[0], crc[1], 0, status,
        ]
    };
    let done = [0, 0, 0, 0, 0, 0, 0, 0x40];
    assert_eq!(serial_from(&[part(0x40, check), done]).unwrap(), 7);
    // Another checksum.
    let damaged = serial_from(&[part(0x40, [0, 0]), done]);
    assert!(matches!(damaged, Err(OtpError::Damaged)));
    // The pending flag gone before the end.
    let broken = serial_from(&[part(0x40, check), [0; 8]]);
    assert!(matches!(broken, Err(OtpError::Damaged)));
    // More parts than the response has.
    let long = serial_from(&[part(0x40, check), part(0x41, check), part(0x42, check)]);
    assert!(matches!(long, Err(OtpError::Damaged)));
}

#[test]
fn the_yubikeys_otp_interfaces_are_found_among_the_hid_devices() {
    let fake = plugged(key());
    let other = FakeSystem::new();
    assert!(!has_hidraw(&other.files));
    assert!(devices(&other.files).is_empty());
    // A keyboard of another vendor.
    fake.files
        .file(
            "/sys/class/hidraw/hidraw0/device/uevent",
            "HID_ID=0003:0000046D:0000C31C\n",
        )
        .file(
            "/sys/class/hidraw/hidraw0/device/report_descriptor",
            [0x05, 0x01, 0x09, 0x06],
        );
    fake.plug_yubikey("hidraw3", key());

    assert!(has_hidraw(&fake.files));
    let found = devices(&fake.files);
    assert_eq!(
        found,
        [PathBuf::from("/dev/hidraw1"), PathBuf::from("/dev/hidraw3")]
    );
    assert!(!is_yubico("HID_ID=0003"));
    assert!(!is_keyboard(&[]));
}

#[test]
fn a_fake_hid_without_the_key_opens_nothing() {
    let hid = FakeHid::default();
    assert!(hid.open(Path::new(DEVICE)).is_err());
    assert!(hid.challenges(DEVICE).is_empty());
    assert!(!hid.is_idle(DEVICE));
}

#[test]
fn a_key_that_stops_waiting_for_its_touch_was_not_touched() {
    // The two parts of the command taken, a wait for the touch, then
    // nothing: the key gave up waiting itself.
    let touch = [0, 0, 0, 0, 0, 0, 0, TOUCH_FLAG | 15];
    let hid = Scripted(Mutex::new(vec![[0; 8], [0; 8], touch, [0; 8]]));
    let fake = FakeSystem::new();

    let error =
        Otp::new(&hid, &fake.clock).response(Path::new(DEVICE), 2, &Secret::copy_from(b"c"));

    assert!(matches!(error, Err(OtpError::NotTouched)), "{error:?}");
    assert!(fake.clock.now().saturating_sub(START) < TOUCH_WAIT);
}
