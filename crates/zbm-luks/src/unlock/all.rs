//! Unlocking every volume of the configuration, or every one discovery
//! finds.

use crate::mappings::Origin;
use std::collections::BTreeSet;

use zbm_config::spec::cmdline::luks::{LUKS, LUKS_KEY};

use crate::config::{Mode, Options, load_config};
use crate::device::{is_block_device, resolve_device};
use crate::error::LuksError;
use crate::mappings::Mapping;
use crate::unlocker::Unlocker;

use super::UnlockSummary;

/// The volumes unlocked so far, and those that could not be.
#[derive(Clone, Copy, Debug, Default)]
struct Tally {
    unlocked: i64,
    failed: i64,
}

impl Tally {
    /// Counts the outcome of opening one volume.
    fn count<T, E>(&mut self, opened: &Result<T, E>) {
        if opened.is_ok() {
            self.unlocked += 1;
        } else {
            self.failed += 1;
        }
    }
}

/// What the unlock run ends with when there was nothing to do: an empty
/// summary, or on a retry of the import loop, [`LuksError::NothingNew`].
fn nothing_to_do(retry: bool) -> Result<UnlockSummary, LuksError> {
    if retry {
        Err(LuksError::NothingNew)
    } else {
        Ok(UnlockSummary::default())
    }
}

impl Unlocker<'_> {
    /// Unlocks every configured or discovered LUKS volume, and says how
    /// many were unlocked and how many were not.
    ///
    /// # Errors
    ///
    /// [`LuksError::NoneUnlocked`] when volumes needed unlocking and none
    /// was; on a retry of the import loop, [`LuksError::NothingNew`] unless
    /// something new was unlocked, or the loop would retry the import forever
    /// without offering the recovery shell.
    pub fn unlock_all(&mut self) -> Result<UnlockSummary, LuksError> {
        let retry = self.settings().retry;
        if !self.available() {
            zbm_trace::debug!("cryptsetup is not available; LUKS support disabled");
            return nothing_to_do(retry);
        }
        let mode = self.load_mode();
        if mode == Mode::Disabled {
            return nothing_to_do(retry);
        }

        // The default key for volumes without one of their own.
        let global_key = self.option(&LUKS_KEY).unwrap_or_default();
        self.settle();
        self.forget_secrets();
        let mut tally = if mode == Mode::Auto {
            let Some(tally) = self.unlock_discovered(&global_key, retry) else {
                return nothing_to_do(retry);
            };
            tally
        } else {
            self.unlock_configured(&mode, &global_key, retry)
        };
        self.forget_secrets();
        // udev creates the /dev/disk/by-* links of the new mappings.
        self.settle();
        tally.unlocked -= self.close_without_zfs();

        let Tally { unlocked, failed } = tally;
        zbm_trace::info!("LUKS: {unlocked} volume(s) available, {failed} not unlocked");
        if unlocked > 0 || (!retry && failed == 0) {
            Ok(UnlockSummary { unlocked, failed })
        } else if retry {
            Err(LuksError::NothingNew)
        } else {
            Err(LuksError::NoneUnlocked { failed })
        }
    }

    /// Which volumes to unlock, from `zbm.luks` and the crypttab of the
    /// image, logging how it decided.
    fn load_mode(&self) -> Mode {
        let zbm_luks = self.option(&LUKS);
        let crypttab_path = self.settings().crypttab.clone();
        let crypttab = self
            .system()
            .files
            .read(&crypttab_path)
            .ok()
            .map(|text| String::from_utf8_lossy(&text).into_owned());
        let (mode, messages) = load_config(
            zbm_luks.as_deref(),
            crypttab.as_deref(),
            &crypttab_path.to_string_lossy(),
        );
        super::log_all(&messages);
        mode
    }

    /// Unlocks every LUKS volume discovery finds, with `global_key` as the
    /// key file; `None` when there is none to unlock.
    fn unlock_discovered(&mut self, global_key: &str, retry: bool) -> Option<Tally> {
        let mut devices = self.discover_devices();
        // On retries, only volumes that appeared since the last pass matter.
        if retry {
            devices.retain(|device| !self.retry_ignores(device));
        }
        if devices.is_empty() {
            zbm_trace::debug!("no LUKS volumes found");
            return None;
        }
        zbm_trace::info!("discovered LUKS volumes: {}", devices.join(" "));
        let mut tally = Tally::default();
        for device in &devices {
            let opened = self.open_device("", device, global_key, "", Origin::Discovered);
            tally.count(&opened);
        }
        Some(tally)
    }

    /// Unlocks the volumes of the crypttab or of `zbm.luks`, each with its
    /// key file or `global_key`.
    fn unlock_configured(&mut self, mode: &Mode, global_key: &str, retry: bool) -> Tally {
        let mut tally = Tally::default();
        let mut seen = BTreeSet::new();
        for entry in mode.entries() {
            let key = if entry.keyfile.is_empty() {
                global_key
            } else {
                &entry.keyfile
            };
            let (options, messages) = Options::parse(&entry.options, self.settings().default_tries);
            super::log_all(&messages);
            if options.noauto {
                zbm_trace::debug!("not unlocking {} (noauto)", entry.spec);
                continue;
            }
            let Some(device) = self.configured_device(&entry.spec, &options, retry, &mut tally)
            else {
                continue;
            };
            if !seen.insert(device.clone()) {
                zbm_trace::warn!("LUKS device {device} is configured more than once");
                continue;
            }
            let opened = self.open_device(
                &entry.name,
                &device,
                key,
                &entry.options,
                Origin::Configured,
            );
            tally.count(&opened);
        }
        tally
    }

    /// The device of the crypttab entry `spec`, waiting for it to appear
    /// unless `retry`: retries never wait, and a device still missing is
    /// checked again next time. A device that does not appear counts as a
    /// failure unless the entry has `nofail`.
    fn configured_device(
        &mut self,
        spec: &str,
        options: &Options,
        retry: bool,
        tally: &mut Tally,
    ) -> Option<String> {
        if retry {
            let system = *self.system();
            return resolve_device(&system, spec).filter(|device| !self.retry_ignores(device));
        }
        let device = self.wait_device(spec, options.nofail);
        if device.is_none() && !options.nofail {
            tally.failed += 1;
        }
        device
    }

    /// Closes the discovered volumes that clearly do not hold ZFS, and says
    /// how many it closed.
    fn close_without_zfs(&mut self) -> i64 {
        let Some(mappings) = self.mappings().read() else {
            return 0;
        };
        let mut closed = 0;
        let mut keep: Vec<Mapping> = Vec::new();
        for mapping in mappings {
            let name = mapping.name.as_str();
            if mapping.auto == "1"
                && is_block_device(self.system().files, &self.mapper(name))
                && !self.may_hold_zfs(name)
            {
                zbm_trace::info!(
                    "closing {name}: {} does not contain a ZFS vdev",
                    mapping.device
                );
                if self.cryptsetup_close(name).is_ok() {
                    closed += 1;
                    continue;
                }
                zbm_trace::warn!("unable to close {name}");
            }
            keep.push(mapping);
        }
        self.mappings().write(&keep);
        closed
    }
}
