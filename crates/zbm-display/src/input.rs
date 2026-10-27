//! Keys from the keyboards' input devices (`/dev/input/event*`), as the
//! kernel reports them to every program that reads them, without the
//! virtual terminal.

#![allow(
    clippy::disallowed_methods,
    reason = "the input devices are this module's system interface: it opens them and reads sysfs"
)]

use std::fs::{File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::keys::{Decoder, KeyPress};
use crate::layout::{Layout, Source};

/// `EV_SYN`: the end of a group of events, or the news that some were lost.
const EV_SYN: u16 = 0;

/// `SYN_REPORT`: the end of a group of events.
const SYN_REPORT: u16 = 0;

/// `SYN_DROPPED`: the device's queue overflowed and events were lost.
const SYN_DROPPED: u16 = 3;

/// `EV_KEY`: an event of a key or button.
const EV_KEY: u16 = 1;

/// `EV_LED`: a light of the keyboard, written to the device.
const EV_LED: u16 = 0x11;

/// `LED_NUML`: the Num Lock light.
const LED_NUML: u16 = 0;

/// `LED_CAPSL`: the Caps Lock light.
const LED_CAPSL: u16 = 1;

/// `LED_SCROLLL`: the Scroll Lock light.
const LED_SCROLLL: u16 = 2;

/// How many events are read at once.
const EVENTS: usize = 32;

/// How many presses are kept between reads: one read makes at most two of
/// each event, a dead key's accent and the key.
const PENDING: usize = 2 * EVENTS;

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

/// What wiped events are overwritten with.
const BLANK_EVENT: InputEvent = InputEvent {
    kind: 0,
    code: 0,
    value: 0,
};

/// What wiped key presses are overwritten with.
const BLANK_PRESS: KeyPress = KeyPress::BLANK;

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

/// Whether the Num Lock light of the input device at `device`
/// (`/dev/input/event3`) is lit, as sysfs tells it
/// (`class/input/event3/device/input3::numlock/brightness`).
fn is_num_lock_lit(sysfs: &Path, device: &Path) -> bool {
    let Some(name) = device.file_name() else {
        return false;
    };
    let parent = sysfs.join("class/input").join(name).join("device");
    std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with("::numlock"))
        .any(|entry| {
            std::fs::read_to_string(entry.path().join("brightness"))
                .is_ok_and(|brightness| brightness.trim() != "0")
        })
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

/// An open keyboard.
#[derive(Debug)]
struct Device {
    file: File,
    /// Where it was opened, to know it when looking for new keyboards.
    path: Option<PathBuf>,
    /// Events were lost: the rest of the group is skipped and the keys
    /// down are read from the device at its end.
    lost: bool,
    /// Its USB vendor and product, for a layout of its own.
    id: Option<DeviceId>,
    /// Its own decoder, with the layout of its own; the keyboards' shared
    /// one otherwise.
    own: Option<Decoder>,
    /// The states of Caps Lock, Num Lock and Scroll Lock its lights show;
    /// `None` when they are not known yet.
    lights: Option<[bool; 3]>,
}

impl Device {
    /// The keyboard `file`, opened at `path`, of `id`, whose lights show
    /// `lights`.
    fn new(
        file: File,
        path: Option<PathBuf>,
        id: Option<DeviceId>,
        lights: Option<[bool; 3]>,
    ) -> Device {
        Device {
            file,
            path,
            lost: false,
            id,
            own: None,
            lights,
        }
    }
}

/// Opens the input device at `path`, for writing as well when it may be,
/// to set its lights.
fn open_device(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .or_else(|_| File::open(path))
        .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", path.display())))
}

/// The record of an `EV_LED` event and the `SYN_REPORT` after it.
fn led_records(code: u16, on: bool) -> Vec<u8> {
    let mut records = vec![0_u8; 16];
    records.extend_from_slice(&EV_LED.to_ne_bytes());
    records.extend_from_slice(&code.to_ne_bytes());
    records.extend_from_slice(&i32::from(on).to_ne_bytes());
    records.resize(2 * EVENT_SIZE, 0);
    records
}

/// The keyboards' input devices, read together. A keyboard that goes away
/// is dropped; one that comes is found by [`Keyboards::rescan`].
#[derive(Debug)]
pub struct Keyboards {
    devices: Vec<Device>,
    /// Where to look for keyboards: `/sys` and `/dev`.
    places: Option<(PathBuf, PathBuf)>,
    decoder: Decoder,
    pending: Vec<KeyPress>,
    /// The layouts of the keyboards that have one of their own.
    rules: Vec<(DeviceId, Source)>,
    /// The compose key, which the keyboards' own decoders get too.
    compose_key: Option<u16>,
    /// The key repeat asked for, in milliseconds: the delay and the period,
    /// which keyboards that come later get too.
    repeat: Option<(u32, u32)>,
}

impl Keyboards {
    /// Opens the keyboards that `sysfs` lists.
    ///
    /// # Errors
    ///
    /// When there is none, or none can be opened.
    pub fn open(sysfs: &Path, dev: &Path) -> io::Result<Keyboards> {
        let (found, failed) = Keyboards::opened(sysfs, dev);
        match failed {
            Some(error) if found.devices.is_empty() => Err(error),
            None if found.devices.is_empty() => {
                Err(io::Error::new(io::ErrorKind::NotFound, "no keyboard found"))
            }
            _ => Ok(found),
        }
    }

    /// Opens the keyboards that `sysfs` lists, if there are any: a USB
    /// keyboard may enumerate after the caller starts, and
    /// [`Keyboards::rescan`] adds it then.
    pub fn find(sysfs: &Path, dev: &Path) -> Keyboards {
        Keyboards::opened(sysfs, dev).0
    }

    /// The keyboards that `sysfs` lists and could be opened, and the last
    /// error of those that could not.
    fn opened(sysfs: &Path, dev: &Path) -> (Keyboards, Option<io::Error>) {
        let mut found = Keyboards::from_files(Vec::new());
        found.places = Some((sysfs.to_owned(), dev.to_owned()));
        let mut failed = None;
        let mut num_lock = false;
        for path in keyboards(sysfs, dev) {
            num_lock |= is_num_lock_lit(sysfs, &path);
            let id = DeviceId::of(sysfs, &path);
            match open_device(&path) {
                Ok(file) => found.devices.push(Device::new(file, Some(path), id, None)),
                Err(error) => failed = Some(error),
            }
        }
        // Num Lock as the keyboards showed it: the firmware or the kernel
        // console may have turned it on.
        found.decoder.set_num_lock(num_lock);
        for device in &mut found.devices {
            device.lights = Some([false, num_lock, false]);
        }
        (found, failed)
    }

    /// Keyboards read from `devices`, open input devices or, in tests,
    /// anything that gives their records.
    pub fn from_files(devices: Vec<File>) -> Keyboards {
        Keyboards {
            devices: devices
                .into_iter()
                .map(|file| Device::new(file, None, None, Some([false; 3])))
                .collect(),
            places: None,
            decoder: Decoder::new(),
            pending: Vec::with_capacity(PENDING),
            rules: Vec::new(),
            compose_key: None,
            repeat: None,
        }
    }

    /// Opens the keyboards that came since they were opened, or since the
    /// last look: plugged in, or found late by the kernel. Returns how many.
    pub fn rescan(&mut self) -> usize {
        let Some((sysfs, dev)) = &self.places else {
            return 0;
        };
        let mut added = 0;
        for path in keyboards(sysfs, dev) {
            let known = self
                .devices
                .iter()
                .any(|device| device.path.as_ref() == Some(&path));
            if !known && let Ok(file) = open_device(&path) {
                if let Some((delay, period)) = self.repeat {
                    let _ = zbm_sys::console::set_key_repeat(file.as_raw_fd(), delay, period);
                }
                let id = DeviceId::of(sysfs, &path);
                self.devices.push(Device::new(file, Some(path), id, None));
                added += 1;
            }
        }
        if added > 0 {
            for index in self.devices.len() - added..self.devices.len() {
                self.adopt(index);
            }
            // The new keyboards show the locks.
            self.show_locks();
        }
        added
    }

    /// How many keyboards are open.
    pub fn count(&self) -> usize {
        self.devices.len()
    }

    /// Turns Num Lock on or off, on every keyboard (`zbm.numlock`).
    pub fn set_num_lock(&mut self, on: bool) {
        self.decoder.set_num_lock(on);
        for device in &mut self.devices {
            if let Some(own) = &mut device.own {
                own.set_num_lock(on);
            }
        }
        self.show_locks();
    }

    /// Has the keyboards repeat a key held for `delay` milliseconds every
    /// `period` milliseconds, those that come later too.
    pub fn set_repeat(&mut self, delay: u32, period: u32) {
        self.repeat = Some((delay, period));
        for device in &self.devices {
            let _ = zbm_sys::console::set_key_repeat(device.file.as_raw_fd(), delay, period);
        }
    }

    /// The decoders: the shared one, and the keyboards' own.
    fn decoders(&self) -> impl Iterator<Item = &Decoder> {
        std::iter::once(&self.decoder).chain(self.devices.iter().filter_map(|d| d.own.as_ref()))
    }

    /// Whether Scroll Lock is on, on a keyboard: the programs' output is
    /// held.
    pub fn is_scroll_lock(&self) -> bool {
        self.decoders().any(Decoder::is_scroll_lock)
    }

    /// Whether Caps Lock is on, on a keyboard.
    pub fn is_caps_lock(&self) -> bool {
        self.decoders().any(Decoder::is_caps_lock)
    }

    /// Whether Num Lock is on, on a keyboard.
    pub fn is_num_lock(&self) -> bool {
        self.decoders().any(Decoder::is_num_lock)
    }

    /// The accent of the dead key typed last, while it waits for the key it
    /// goes on.
    pub fn pending_accent(&self) -> Option<char> {
        self.decoders().find_map(Decoder::pending_accent)
    }

    /// Has key `code` start the compose key's sequences (`None` for none),
    /// on the layouts that have its rules.
    pub fn set_compose_key(&mut self, code: Option<u16>) {
        self.compose_key = code;
        self.decoder.set_compose_key(code);
        for device in &mut self.devices {
            if let Some(own) = &mut device.own {
                own.set_compose_key(code);
            }
        }
    }

    /// Translates keys with `layout` from now on, or on the US layout.
    pub fn set_layout(&mut self, layout: Option<Layout>) {
        self.decoder.set_layout(layout);
    }

    /// The name of the layout keys are translated with.
    pub fn layout(&self) -> &str {
        self.decoder.layout()
    }

    /// The open devices, for a caller that waits on them with other files.
    pub fn fds(&self) -> Vec<i32> {
        self.devices
            .iter()
            .map(|device| device.file.as_raw_fd())
            .collect()
    }

    /// Stops reading the device at `index`, which went away.
    pub fn drop_device(&mut self, index: usize) {
        if index < self.devices.len() {
            self.devices.remove(index);
        }
    }

    /// The presses pending, handed over; the next ones go to a new vector
    /// of the same capacity, so that none grows and leaves a copy.
    fn take_pending(&mut self) -> Vec<KeyPress> {
        std::mem::replace(&mut self.pending, Vec::with_capacity(PENDING))
    }

    /// The keys down on every keyboard, as the kernel knows them.
    fn keys_down(&self) -> Vec<u16> {
        let mut down: Vec<u16> = self
            .devices
            .iter()
            .filter_map(|device| zbm_sys::console::keys_down(device.file.as_raw_fd()).ok())
            .flatten()
            .collect();
        down.sort_unstable();
        down.dedup();
        down
    }

    /// Turns the events of the device at `index` into presses.
    fn take_events(&mut self, index: usize, events: &[InputEvent]) {
        for event in events {
            let Some(device) = self.devices.get_mut(index) else {
                return;
            };
            match (event.kind, event.code) {
                (EV_SYN, SYN_DROPPED) => device.lost = true,
                (EV_SYN, SYN_REPORT) if device.lost => {
                    device.lost = false;
                    let down = self.keys_down();
                    self.decoder_of(index).resync(&down);
                }
                _ if device.lost => {}
                (EV_KEY, code) if self.pending.len() + 2 <= PENDING => {
                    let decoder = match self.devices.get_mut(index).and_then(|d| d.own.as_mut()) {
                        Some(own) => own,
                        None => &mut self.decoder,
                    };
                    decoder.feed_into(code, event.value, &mut self.pending);
                }
                _ => {}
            }
        }
    }

    /// Lights Caps Lock, Num Lock and Scroll Lock on every keyboard when
    /// its decoder's changed: the kernel does not, while the console reads
    /// the keys.
    fn show_locks(&mut self) {
        for device in &mut self.devices {
            let decoder = device.own.as_ref().unwrap_or(&self.decoder);
            let now = [
                decoder.is_caps_lock(),
                decoder.is_num_lock(),
                decoder.is_scroll_lock(),
            ];
            if device.lights == Some(now) {
                continue;
            }
            device.lights = Some(now);
            let mut records = led_records(LED_CAPSL, now[0]);
            records.extend(led_records(LED_NUML, now[1]));
            records.extend(led_records(LED_SCROLLL, now[2]));
            let _ = device.file.write_all(&records);
        }
    }

    /// Reads the events of the device at `index`, which has some, and
    /// returns the presses among them, with any still pending. A device
    /// that went away (`ENODEV`, or any other error, or the end of its
    /// records) is dropped: the keys come from the others.
    ///
    /// # Errors
    ///
    /// None at present; kept for the callers that wait on devices.
    pub fn read_device(&mut self, index: usize) -> io::Result<Vec<KeyPress>> {
        let mut buffer = [0_u8; EVENT_SIZE * EVENTS];
        let Some(device) = self.devices.get_mut(index) else {
            return Ok(self.take_pending());
        };
        match device.file.read(&mut buffer) {
            Ok(0) => self.drop_device(index),
            Ok(length) => {
                let mut events = parse_events(buffer.get(..length).unwrap_or(&[]));
                self.take_events(index, &events);
                // The key codes of a passphrase tell it as well as its bytes.
                zbm_sys::wipe_values(&mut events, BLANK_EVENT);
                self.show_locks();
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) => {}
            Err(_) => self.drop_device(index),
        }
        zbm_sys::wipe(&mut buffer);
        Ok(self.take_pending())
    }
}

impl KeySource for Keyboards {
    fn next(&mut self, timeout: Duration) -> io::Result<Option<KeyPress>> {
        if self.pending.is_empty() {
            match zbm_sys::console::first_readable(&self.fds(), Some(timeout))? {
                Some(index) => self.pending = self.read_device(index)?,
                // Nothing typed for a while: a keyboard may have come.
                None => {
                    self.rescan();
                }
            }
        }
        let press = (!self.pending.is_empty()).then(|| self.pending.remove(0));
        if self.pending.is_empty() {
            // `remove` leaves copies of the presses in the spare capacity.
            zbm_sys::wipe_values(&mut self.pending, BLANK_PRESS);
        }
        Ok(press)
    }
}

mod own;

pub use own::DeviceId;

#[cfg(test)]
mod tests;
