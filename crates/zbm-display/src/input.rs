//! Keys from the keyboards' input devices (`/dev/input/event*`), as the
//! kernel reports them to every program that reads them, without the
//! virtual terminal.

#![allow(
    clippy::disallowed_methods,
    reason = "the input devices are this module's system interface: it opens them and reads sysfs"
)]

use std::fs::File;
use std::io::{self, Read as _};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::keys::{Decoder, KeyPress};

/// `EV_KEY`: an event of a key or button.
const EV_KEY: u16 = 1;

/// The size of `struct input_event` on 64-bit Linux: a `struct timeval` of
/// two 64-bit fields, then the type, the code and the value.
pub const EVENT_SIZE: usize = 24;

/// Keys a keyboard has and a mouse, a power button or a lid switch do not:
/// `KEY_A`, `KEY_Z`, `KEY_ENTER` and `KEY_SPACE`.
const KEYBOARD_KEYS: [u32; 4] = [30, 44, 28, 57];

/// One event of an input device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputEvent {
    /// The type: `EV_KEY` for keys.
    pub kind: u16,
    /// The key code, for keys.
    pub code: u16,
    /// 0 released, 1 pressed, 2 repeated, for keys.
    pub value: i32,
}

/// The events in `bytes`, as read from an input device; a partial event at
/// the end is left out.
pub fn parse_events(bytes: &[u8]) -> Vec<InputEvent> {
    bytes
        .chunks_exact(EVENT_SIZE)
        .filter_map(|chunk| <&[u8; EVENT_SIZE]>::try_from(chunk).ok())
        .map(|event| InputEvent {
            kind: u16::from_ne_bytes([event[16], event[17]]),
            code: u16::from_ne_bytes([event[18], event[19]]),
            value: i32::from_ne_bytes([event[20], event[21], event[22], event[23]]),
        })
        .collect()
}

/// Whether bit `bit` is set in a sysfs capability bitmap: hexadecimal
/// words of 64 bits, the most significant first (`capabilities/key`).
fn has_bit(bitmap: &str, bit: u32) -> bool {
    let words: Vec<&str> = bitmap.split_whitespace().rev().collect();
    let (index, shift) = (usize::try_from(bit / 64).unwrap_or(usize::MAX), bit % 64);
    words
        .get(index)
        .and_then(|word| u64::from_str_radix(word, 16).ok())
        .is_some_and(|word| word >> shift & 1 == 1)
}

/// Whether a device with the key capabilities `keys` is a keyboard.
fn is_keyboard(keys: &str) -> bool {
    KEYBOARD_KEYS.iter().all(|&key| has_bit(keys, key))
}

/// The event devices of the keyboards that `sysfs` (`/sys`) lists, as
/// paths below `dev` (`/dev`), sorted.
pub fn keyboards(sysfs: &Path, dev: &Path) -> Vec<PathBuf> {
    let class = sysfs.join("class/input");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&class)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with("event"))
        .filter(|name| {
            let keys = class.join(name).join("device/capabilities/key");
            std::fs::read_to_string(keys).is_ok_and(|keys| is_keyboard(&keys))
        })
        .map(|name| dev.join("input").join(name))
        .collect();
    found.sort();
    found
}

/// Where key presses come from.
pub trait KeySource {
    /// The next key press, waiting at most `timeout`; `None` when the time
    /// ran out.
    ///
    /// # Errors
    ///
    /// When the keys cannot be read.
    fn next(&mut self, timeout: Duration) -> io::Result<Option<KeyPress>>;
}

/// The keyboards' input devices, read together.
#[derive(Debug)]
pub struct Keyboards {
    devices: Vec<File>,
    decoder: Decoder,
    pending: Vec<KeyPress>,
}

impl Keyboards {
    /// Opens the keyboards that `sysfs` lists.
    ///
    /// # Errors
    ///
    /// When there is none, or one cannot be opened.
    pub fn open(sysfs: &Path, dev: &Path) -> io::Result<Keyboards> {
        let paths = keyboards(sysfs, dev);
        if paths.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no keyboard found"));
        }
        let devices = paths
            .iter()
            .map(|path| {
                File::open(path).map_err(|error| {
                    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
                })
            })
            .collect::<io::Result<Vec<File>>>()?;
        Ok(Keyboards::from_files(devices))
    }

    /// Keyboards read from `devices`, open input devices or, in tests,
    /// anything that gives their records.
    pub fn from_files(devices: Vec<File>) -> Keyboards {
        Keyboards {
            devices,
            decoder: Decoder::new(),
            pending: Vec::new(),
        }
    }

    /// The open devices, for a caller that waits on them with other files.
    pub fn fds(&self) -> Vec<i32> {
        self.devices.iter().map(AsRawFd::as_raw_fd).collect()
    }

    /// Reads the events of the device at `index`, which has some, and
    /// returns the presses among them, with any still pending.
    ///
    /// # Errors
    ///
    /// When the device cannot be read.
    pub fn read_device(&mut self, index: usize) -> io::Result<Vec<KeyPress>> {
        let mut buffer = [0_u8; EVENT_SIZE * 32];
        if let Some(device) = self.devices.get_mut(index) {
            let length = device.read(&mut buffer)?;
            for event in parse_events(buffer.get(..length).unwrap_or(&[])) {
                if event.kind == EV_KEY
                    && let Some(press) = self.decoder.feed(event.code, event.value)
                {
                    self.pending.push(press);
                }
            }
        }
        Ok(std::mem::take(&mut self.pending))
    }
}

impl KeySource for Keyboards {
    fn next(&mut self, timeout: Duration) -> io::Result<Option<KeyPress>> {
        if self.pending.is_empty()
            && let Some(index) = zbm_sys::console::first_readable(&self.fds(), Some(timeout))?
        {
            self.pending = self.read_device(index)?;
        }
        Ok((!self.pending.is_empty()).then(|| self.pending.remove(0)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn event(kind: u16, code: u16, value: i32) -> Vec<u8> {
        let mut bytes = vec![0_u8; 16];
        bytes.extend_from_slice(&kind.to_ne_bytes());
        bytes.extend_from_slice(&code.to_ne_bytes());
        bytes.extend_from_slice(&value.to_ne_bytes());
        bytes
    }

    #[test]
    fn events_are_read_from_whole_records() {
        let mut bytes = event(EV_KEY, 30, 1);
        bytes.extend(event(0, 0, 0));
        bytes.extend([1, 2, 3]);

        let events = parse_events(&bytes);

        assert_eq!(
            events,
            [
                InputEvent {
                    kind: EV_KEY,
                    code: 30,
                    value: 1
                },
                InputEvent {
                    kind: 0,
                    code: 0,
                    value: 0
                },
            ]
        );
    }

    /// The `capabilities/key` of an AT keyboard (QEMU's PS/2 keyboard) and
    /// of a power button.
    const AT_KEYBOARD: &str = "402000000 3803078f800d001 feffffdfffefffff fffffffffffffffe";
    const POWER_BUTTON: &str = "10000000000000 0";

    #[test]
    fn keyboards_are_told_from_other_devices_by_their_keys() {
        assert!(has_bit(AT_KEYBOARD, 30));
        assert!(has_bit(AT_KEYBOARD, 64 + 1));
        assert!(!has_bit(AT_KEYBOARD, 0));
        assert!(!has_bit(AT_KEYBOARD, 64 * 9));
        assert!(!has_bit("zz", 1));
        assert!(is_keyboard(AT_KEYBOARD));
        assert!(!is_keyboard(POWER_BUTTON));
        assert!(!is_keyboard(""));
    }

    #[test]
    fn keyboards_are_found_in_sysfs() {
        let root = std::env::temp_dir().join(format!("zbm-display-input-{}", std::process::id()));
        for (name, keys) in [
            ("event0", POWER_BUTTON),
            ("event3", AT_KEYBOARD),
            ("mouse0", AT_KEYBOARD),
        ] {
            let caps = root
                .join("sys/class/input")
                .join(name)
                .join("device/capabilities");
            std::fs::create_dir_all(&caps).unwrap();
            std::fs::write(caps.join("key"), format!("{keys}\n")).unwrap();
        }

        let found = keyboards(&root.join("sys"), Path::new("/dev"));
        let missing = Keyboards::open(&root.join("nothing"), Path::new("/dev"));

        assert_eq!(found, [PathBuf::from("/dev/input/event3")]);
        assert_eq!(missing.unwrap_err().kind(), io::ErrorKind::NotFound);
        let unopenable = Keyboards::open(&root.join("sys"), &root.join("no-dev"));
        assert!(unopenable.unwrap_err().to_string().contains("event3"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn presses_are_read_from_the_devices_that_have_events() {
        let root = std::env::temp_dir().join(format!("zbm-display-keys-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut bytes = event(EV_KEY, 42, 1);
        for (kind, code, value) in [(EV_KEY, 30, 1), (0, 0, 0), (EV_KEY, 30, 0), (EV_KEY, 48, 1)] {
            bytes.extend(event(kind, code, value));
        }
        std::fs::write(root.join("events"), bytes).unwrap();
        let mut keyboards = Keyboards {
            devices: vec![File::open(root.join("events")).unwrap()],
            decoder: Decoder::new(),
            pending: Vec::new(),
        };

        let first = keyboards.next(Duration::ZERO).unwrap().unwrap();
        let second = keyboards.next(Duration::ZERO).unwrap().unwrap();

        assert_eq!(first.key, crate::keys::Key::Char('A'));
        assert_eq!(second.key, crate::keys::Key::Char('B'));
        assert_eq!(keyboards.next(Duration::ZERO).unwrap(), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
