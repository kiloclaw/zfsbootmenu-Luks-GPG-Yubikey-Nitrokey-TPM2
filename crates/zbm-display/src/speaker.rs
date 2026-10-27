//! The PC speaker, for the console's audible bell: an input device that
//! takes `EV_SND` events, which the `pcspkr` driver gives.

#![allow(
    clippy::disallowed_methods,
    reason = "the speaker's input device is this module's system interface: it reads sysfs"
)]

use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::path::Path;

use crate::input::EVENT_SIZE;

/// `EV_SND`.
const EV_SND: u16 = 0x12;

/// `SND_TONE`: a tone of so many hertz, 0 to stop it.
const SND_TONE: u16 = 0x02;

/// The name the `pcspkr` driver gives its input device.
const NAME: &str = "PC Speaker";

/// The PC speaker's input device, open for writing. A tone still sounding
/// when it is dropped stops: the console may end, or hand the display back
/// before `kexec`, in the middle of a bell.
#[derive(Debug)]
pub struct Speaker {
    file: File,
    /// Whether the last tone written was not a stop.
    sounding: bool,
}

impl Speaker {
    /// The PC speaker among the input devices in `sysfs`, opened in `dev`;
    /// `None` when there is none, or it cannot be opened.
    pub fn find(sysfs: &Path, dev: &Path) -> Option<Speaker> {
        let class = sysfs.join("class/input");
        let mut names: Vec<String> = std::fs::read_dir(&class)
            .ok()?
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| name.starts_with("event"))
            .collect();
        names.sort();
        let name = names.into_iter().find(|name| {
            let path = class.join(name).join("device/name");
            std::fs::read_to_string(path).is_ok_and(|text| text.trim() == NAME)
        })?;
        let file = OpenOptions::new()
            .write(true)
            .open(dev.join("input").join(name))
            .ok()?;
        Some(Speaker {
            file,
            sounding: false,
        })
    }

    /// Sounds a tone of `hertz`, until the next call; 0 stops it.
    ///
    /// # Errors
    ///
    /// When the device cannot be written.
    pub fn tone(&mut self, hertz: u16) -> io::Result<()> {
        self.file.write_all(&tone_records(hertz))?;
        self.sounding = hertz != 0;
        Ok(())
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        if self.sounding {
            let _ = self.tone(0);
        }
    }
}

/// The record of a `SND_TONE` event and the `SYN_REPORT` after it.
fn tone_records(hertz: u16) -> Vec<u8> {
    let mut records = vec![0_u8; 16];
    records.extend_from_slice(&EV_SND.to_ne_bytes());
    records.extend_from_slice(&SND_TONE.to_ne_bytes());
    records.extend_from_slice(&i32::from(hertz).to_ne_bytes());
    records.resize(2 * EVENT_SIZE, 0);
    records
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn a_tone_is_an_event_and_a_report() {
        let records = tone_records(750);
        assert_eq!(records.len(), 48);
        assert_eq!(&records[16..18], &EV_SND.to_ne_bytes());
        assert_eq!(&records[18..20], &SND_TONE.to_ne_bytes());
        assert_eq!(&records[20..24], &750_i32.to_ne_bytes());
        assert!(records[24..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn the_speaker_is_found_by_its_name_and_written() {
        let root = std::env::temp_dir().join(format!("zbm-speaker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (sysfs, dev) = (root.join("sys"), root.join("dev"));
        for (event, name) in [("event0", "AT Keyboard"), ("event3", "PC Speaker")] {
            let device = sysfs.join("class/input").join(event).join("device");
            std::fs::create_dir_all(&device).unwrap();
            std::fs::write(device.join("name"), format!("{name}\n")).unwrap();
        }
        std::fs::create_dir_all(sysfs.join("class/input/input3")).unwrap();
        std::fs::create_dir_all(dev.join("input")).unwrap();
        // Not openable yet: none.
        assert!(Speaker::find(&sysfs, &dev).is_none());
        std::fs::write(dev.join("input/event3"), b"").unwrap();
        let mut speaker = Speaker::find(&sysfs, &dev).unwrap();
        speaker.tone(750).unwrap();
        speaker.tone(0).unwrap();
        let written = std::fs::read(dev.join("input/event3")).unwrap();
        assert_eq!(written.len(), 96);
        assert!(Speaker::find(&root, &dev).is_none());

        // Dropped while sounding, it stops the tone; dropped silent, it
        // writes nothing.
        drop(speaker);
        assert_eq!(std::fs::read(dev.join("input/event3")).unwrap().len(), 96);
        let mut speaker = Speaker::find(&sysfs, &dev).unwrap();
        speaker.tone(750).unwrap();
        drop(speaker);
        // The device is not opened to append: the second speaker wrote its
        // tone and its stop from the start.
        let written = std::fs::read(dev.join("input/event3")).unwrap();
        assert_eq!(&written[20..24], &750_i32.to_ne_bytes());
        assert_eq!(&written[48 + 20..48 + 24], &0_i32.to_ne_bytes());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
