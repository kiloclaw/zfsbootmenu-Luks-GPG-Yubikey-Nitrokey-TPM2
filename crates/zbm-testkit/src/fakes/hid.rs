//! HID devices: YubiKeys that answer challenge-response over their OTP
//! interface as a real key does, part by part, with their checksums.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use zbm_platform::hid::{FeatureReports, HidDevices, Report};

use crate::sync::lock;

/// What a fake YubiKey's slot answers to a challenge: its response, or
/// `None` when the slot is not set up for challenge-response.
pub type Respond = dyn Fn(u8, &[u8]) -> Option<Vec<u8>> + Send + Sync;

/// A YubiKey's OTP interface.
pub struct FakeYubiKey {
    respond: Box<Respond>,
    serial: Option<u32>,
    touch: usize,
    busy: usize,
    gone: bool,
}

impl FakeYubiKey {
    /// A key whose slots answer with `respond(slot, challenge)`, the
    /// challenge without the padding of its last byte (`HMAC_LT64`).
    pub fn new(
        respond: impl Fn(u8, &[u8]) -> Option<Vec<u8>> + Send + Sync + 'static,
    ) -> FakeYubiKey {
        FakeYubiKey {
            respond: Box::new(respond),
            serial: None,
            touch: 0,
            busy: 0,
            gone: false,
        }
    }

    /// The key, showing the serial number `serial`.
    #[must_use]
    pub fn with_serial(self, serial: u32) -> FakeYubiKey {
        FakeYubiKey {
            serial: Some(serial),
            ..self
        }
    }

    /// The key, waiting for a touch for `reads` reads of its status
    /// before it responds.
    #[must_use]
    pub fn waiting_for_touch(self, reads: usize) -> FakeYubiKey {
        FakeYubiKey {
            touch: reads,
            ..self
        }
    }

    /// The key, keeping its write flag for `reads` reads after each part.
    #[must_use]
    pub fn slow(self, reads: usize) -> FakeYubiKey {
        FakeYubiKey {
            busy: reads,
            ..self
        }
    }

    /// The key, gone: every report fails.
    #[must_use]
    pub fn gone(self) -> FakeYubiKey {
        FakeYubiKey { gone: true, ..self }
    }
}

/// The CRC-16 of `bytes`, as a YubiKey computes it.
fn crc16(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for &byte in bytes {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            let carry = crc & 1;
            crc >>= 1;
            if carry == 1 {
                crc ^= 0x8408;
            }
        }
    }
    crc
}

/// Where a fake key is in the protocol.
#[derive(Default)]
enum State {
    /// Taking the parts of a frame.
    #[default]
    Idle,
    /// Waiting for a touch for so many more reads, then responding.
    Touch(usize, Vec<u8>),
    /// Giving its response, from the part of that number.
    Responding(Vec<u8>, usize),
}

/// A key and where it is.
struct Device {
    key: FakeYubiKey,
    frame: [u8; 70],
    state: State,
    busy: usize,
    /// The challenges it was given, by slot.
    challenges: Vec<(u8, Vec<u8>)>,
}

impl Device {
    /// Takes the whole frame.
    fn command(&mut self) {
        let payload = &self.frame[..64];
        let crc = u16::from_le_bytes([self.frame[65], self.frame[66]]);
        if crc16(payload) != crc {
            return;
        }
        let data = match self.frame[64] {
            command @ (0x30 | 0x38) => {
                let slot = if command == 0x30 { 1 } else { 2 };
                let last = payload[63];
                let end = payload
                    .iter()
                    .rposition(|&byte| byte != last)
                    .map_or(0, |at| at + 1);
                let challenge = payload[..end].to_vec();
                let response = (self.key.respond)(slot, &challenge);
                self.challenges.push((slot, challenge));
                response
            }
            0x10 => self.key.serial.map(|serial| serial.to_be_bytes().to_vec()),
            _ => None,
        };
        let Some(mut data) = data else {
            return;
        };
        let check = !crc16(&data);
        data.extend_from_slice(&check.to_le_bytes());
        self.state = State::Touch(self.key.touch, data);
    }

    /// The report the key gives now.
    fn status(&mut self) -> [u8; 8] {
        let mut report = [0; 8];
        if self.busy > 0 {
            self.busy -= 1;
            report[7] = 0x80;
            return report;
        }
        match std::mem::take(&mut self.state) {
            State::Idle => {}
            State::Touch(0, data) => {
                self.state = State::Responding(data, 0);
                return self.status();
            }
            State::Touch(reads, data) => {
                report[7] = 0x20 | 15;
                self.state = State::Touch(reads - 1, data);
            }
            State::Responding(data, part) => {
                let start = part * 7;
                if start < data.len() {
                    let end = (start + 7).min(data.len());
                    report[..end - start].copy_from_slice(&data[start..end]);
                    report[7] = 0x40 | u8::try_from(part).unwrap_or(0x1f);
                } else {
                    // Every part given: the sequence starts again.
                    report[7] = 0x40;
                }
                self.state = State::Responding(data, part + 1);
            }
        }
        report
    }

    /// Takes the report `report`.
    fn take(&mut self, report: [u8; 8]) {
        let status = report[7];
        if status == 0x8f {
            self.state = State::Idle;
            return;
        }
        if status & 0x80 == 0 {
            return;
        }
        let sequence = usize::from(status & 0x1f);
        if sequence == 0 {
            self.frame = [0; 70];
            self.state = State::Idle;
        }
        if let Some(part) = self.frame.get_mut(sequence * 7..sequence * 7 + 7) {
            part.copy_from_slice(&report[..7]);
        }
        self.busy = self.key.busy;
        if sequence == 9 {
            self.command();
        }
    }
}

/// HID devices: the YubiKeys plugged in, by their device nodes.
#[derive(Clone, Default)]
pub struct FakeHid {
    devices: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<Device>>>>>,
}

impl FakeHid {
    /// Plugs `key` in at the device node `path`.
    pub fn plug(&self, path: impl AsRef<Path>, key: FakeYubiKey) -> &FakeHid {
        let device = Device {
            key,
            frame: [0; 70],
            state: State::Idle,
            busy: 0,
            challenges: Vec::new(),
        };
        lock(&self.devices).insert(path.as_ref().to_owned(), Arc::new(Mutex::new(device)));
        self
    }

    /// The challenges the key at `path` was given, by slot, without their
    /// padding.
    pub fn challenges(&self, path: impl AsRef<Path>) -> Vec<(u8, Vec<u8>)> {
        lock(&self.devices)
            .get(path.as_ref())
            .map(|device| lock(device).challenges.clone())
            .unwrap_or_default()
    }

    /// Whether the key at `path` was left as it was before a command:
    /// reading ended by the dummy report.
    pub fn is_idle(&self, path: impl AsRef<Path>) -> bool {
        lock(&self.devices)
            .get(path.as_ref())
            .is_some_and(|device| matches!(lock(device).state, State::Idle))
    }
}

impl HidDevices for FakeHid {
    fn open(&self, path: &Path) -> io::Result<Box<dyn FeatureReports>> {
        let device = lock(&self.devices).get(path).cloned();
        let device = device.ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok(Box::new(Open(device)))
    }
}

/// An open fake device.
struct Open(Arc<Mutex<Device>>);

impl FeatureReports for Open {
    fn get(&mut self, report: &mut Report) -> io::Result<()> {
        let mut device = lock(&self.0);
        if device.key.gone {
            return Err(io::Error::from_raw_os_error(19));
        }
        report.0 = device.status();
        Ok(())
    }

    fn set(&mut self, report: &Report) -> io::Result<()> {
        let mut device = lock(&self.0);
        if device.key.gone {
            return Err(io::Error::from_raw_os_error(19));
        }
        device.take(report.0);
        Ok(())
    }
}

impl super::FakeSystem {
    /// Plugs `key` in as the `hidraw` device `name` (`hidraw0`), with its
    /// OTP interface's entry in `/sys/class/hidraw` as the kernel makes it,
    /// and the FIDO interface beside it, as `<name>-fido`, which is not
    /// the OTP interface.
    pub fn plug_yubikey(&self, name: &str, key: FakeYubiKey) {
        let uevent = "DRIVER=hid-generic\nHID_ID=0003:00001050:00000407\n\
                      HID_NAME=Yubico YubiKey OTP+FIDO+CCID\n";
        let class = std::path::Path::new("/sys/class/hidraw");
        let keyboard: &[u8] = &[0x05, 0x01, 0x09, 0x06, 0xa1, 0x01];
        let fido: &[u8] = &[0x06, 0xd0, 0xf1, 0x09, 0x01, 0xa1, 0x01];
        for (node, descriptor) in [(name.to_owned(), keyboard), (format!("{name}-fido"), fido)] {
            let device = class.join(&node).join("device");
            self.files
                .file(device.join("uevent"), uevent)
                .file(device.join("report_descriptor"), descriptor)
                .device(format!("/dev/{node}"));
        }
        self.hid.plug(format!("/dev/{name}"), key);
    }
}
