//! What the real host tells the console beyond the programs and the keys:
//! the time and the battery for the status line, and the tone of the
//! audible bell on the PC speaker.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use zbm_display::speaker::Speaker;

use crate::screen::Clock;

/// Where the kernel describes the power supplies.
pub const POWER_SUPPLY: &str = "/sys/class/power_supply";

/// How often the battery is read again.
const BATTERY_EVERY: Duration = Duration::from_secs(60);

/// The bell's tone, in hertz.
const TONE: u16 = 750;

/// How long the bell sounds.
const BEEP: Duration = Duration::from_millis(100);

/// The time of day now, if the clock is past the epoch.
pub fn clock() -> Option<Clock> {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    Some(Clock::of(since.as_secs()))
}

/// The charge in percent of the first battery among the power supplies
/// below `power`, if there is one.
pub fn battery(power: &Path) -> Option<u8> {
    let mut names: Vec<_> = std::fs::read_dir(power)
        .ok()?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .collect();
    names.sort();
    names.into_iter().find_map(|supply| {
        let kind = std::fs::read_to_string(supply.join("type")).ok()?;
        if kind.trim() != "Battery" {
            return None;
        }
        let capacity = std::fs::read_to_string(supply.join("capacity")).ok()?;
        capacity
            .trim()
            .parse::<u8>()
            .ok()
            .map(|charge| charge.min(100))
    })
}

/// The battery's charge, read again at most every [`BATTERY_EVERY`].
#[derive(Debug, Default)]
pub(super) struct Battery {
    read_at: Option<Instant>,
    charge: Option<u8>,
}

impl Battery {
    /// The charge of the battery below `power` at `now`.
    pub fn charge(&mut self, power: &Path, now: Instant) -> Option<u8> {
        let is_old = self
            .read_at
            .is_none_or(|read_at| now.duration_since(read_at) >= BATTERY_EVERY);
        if is_old {
            self.charge = battery(power);
            self.read_at = Some(now);
        }
        self.charge
    }
}

/// The PC speaker, and until when it sounds.
#[derive(Debug, Default)]
pub(super) struct Beeper {
    pub speaker: Option<Speaker>,
    until: Option<Instant>,
}

impl Beeper {
    /// Sounds the bell at `now`, if there is a speaker.
    pub fn ring(&mut self, now: Instant) {
        if let Some(speaker) = &mut self.speaker
            && speaker.tone(TONE).is_ok()
        {
            self.until = Some(now + BEEP);
        }
    }

    /// Stops the tone whose time came at `now`.
    pub fn quiet(&mut self, now: Instant) {
        if self.until.is_some_and(|until| until <= now) {
            self.until = None;
            if let Some(speaker) = &mut self.speaker {
                let _ = speaker.tone(0);
            }
        }
    }

    /// How long until the tone stops, if one sounds.
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.until.map(|until| until.saturating_duration_since(now))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_first_battery_s_charge_is_read_and_kept_a_minute() {
        let dir = zbm_testkit::TempDir::new("zbm-console-power");
        let supply = |name: &str, kind: &str, capacity: &str| {
            let path = dir.join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("type"), format!("{kind}\n")).unwrap();
            std::fs::write(path.join("capacity"), format!("{capacity}\n")).unwrap();
        };
        assert_eq!(battery(dir.path()), None);
        supply("AC", "Mains", "");
        supply("BAT0", "Battery", "87");
        assert_eq!(battery(dir.path()), Some(87));
        assert_eq!(battery(&dir.join("missing")), None);

        let mut cached = Battery::default();
        let start = Instant::now();
        assert_eq!(cached.charge(dir.path(), start), Some(87));
        supply("BAT0", "Battery", "250");
        assert_eq!(
            cached.charge(dir.path(), start + BATTERY_EVERY / 2),
            Some(87)
        );
        assert_eq!(cached.charge(dir.path(), start + BATTERY_EVERY), Some(100));
    }

    #[test]
    fn without_a_speaker_the_bell_is_silent() {
        let mut beeper = Beeper::default();
        let now = Instant::now();
        beeper.ring(now);
        assert_eq!(beeper.remaining(now), None);
        beeper.quiet(now);
        assert!(clock().is_some());
    }
}
