//! Unlocking every volume of the configuration, or every one discovery
//! finds.

use std::collections::BTreeSet;

use crate::config::{Mode, Options, load_config};
use crate::device::{is_block_device, resolve_device};
use crate::error::LuksError;
use crate::mappings::Mapping;
use crate::session::Session;

use super::UnlockSummary;

impl Session<'_> {
    /// Unlocks every configured or discovered LUKS volume
    /// (`luks_unlock_all`), and says how many were unlocked and how many
    /// were not.
    ///
    /// # Errors
    ///
    /// [`LuksError::NoneUnlocked`] when volumes needed unlocking and none
    /// was; on a retry of the import loop, [`LuksError::NothingNew`] unless
    /// something new was unlocked, or the loop would retry the import forever
    /// without offering the recovery shell.
    pub fn unlock_all(&mut self) -> Result<UnlockSummary, LuksError> {
        let retry = self.settings().retry;
        let nothing_to_do = || {
            if retry {
                Err(LuksError::NothingNew)
            } else {
                Ok(UnlockSummary::default())
            }
        };
        if !self.available() {
            zbm_trace::debug!("cryptsetup is not available; LUKS support disabled");
            return nothing_to_do();
        }

        let zbm_luks = self.zbm_arg("zbm.luks");
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
        self.log_all(&messages);
        if mode == Mode::Disabled {
            return nothing_to_do();
        }

        // The default key for volumes without one of their own.
        let global_key = self.zbm_arg("zbm.luks_key").unwrap_or_default();
        self.settle();
        let mut unlocked: i64 = 0;
        let mut failed: i64 = 0;
        self.forget_secrets();

        if mode == Mode::Auto {
            let mut devices = self.discover_devices();
            // On retries, only volumes that appeared since the last pass
            // matter.
            if retry {
                devices.retain(|device| !self.retry_ignores(device));
            }
            if devices.is_empty() {
                zbm_trace::debug!("no LUKS volumes found");
                return nothing_to_do();
            }
            zbm_trace::info!("discovered LUKS volumes: {}", devices.join(" "));
            for device in &devices {
                if self.open_device("", device, &global_key, "", true).is_ok() {
                    unlocked += 1;
                } else {
                    failed += 1;
                }
            }
        } else {
            let mut seen = BTreeSet::new();
            for entry in mode.entries() {
                let key = if entry.keyfile.is_empty() {
                    &global_key
                } else {
                    &entry.keyfile
                };
                let (options, messages) =
                    Options::parse(&entry.options, self.settings().default_tries);
                self.log_all(&messages);
                if options.noauto {
                    zbm_trace::debug!("not unlocking {} (noauto)", entry.spec);
                    continue;
                }

                // Retries never wait: a device still missing is checked again
                // next time.
                let device = if retry {
                    let system = *self.system();
                    match resolve_device(&system, &entry.spec) {
                        Some(device) if !self.retry_ignores(&device) => device,
                        _ => continue,
                    }
                } else {
                    match self.wait_device(&entry.spec, options.nofail) {
                        Some(device) => device,
                        None => {
                            if !options.nofail {
                                failed += 1;
                            }
                            continue;
                        }
                    }
                };

                if !seen.insert(device.clone()) {
                    zbm_trace::warn!("LUKS device {device} is configured more than once");
                    continue;
                }
                if self
                    .open_device(&entry.name, &device, key, &entry.options, false)
                    .is_ok()
                {
                    unlocked += 1;
                } else {
                    failed += 1;
                }
            }
        }

        self.forget_secrets();
        // udev creates the /dev/disk/by-* links of the new mappings.
        self.settle();

        // Discovered volumes that clearly do not hold ZFS are closed again.
        if let Some(mappings) = self.read_mappings() {
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
                        unlocked -= 1;
                        continue;
                    }
                    zbm_trace::warn!("unable to close {name}");
                }
                keep.push(mapping);
            }
            self.write_mappings(&keep);
        }

        zbm_trace::info!("LUKS: {unlocked} volume(s) available, {failed} not unlocked");
        let summary = UnlockSummary { unlocked, failed };
        if unlocked > 0 || (!retry && failed == 0) {
            Ok(summary)
        } else if retry {
            Err(LuksError::NothingNew)
        } else {
            Err(LuksError::NoneUnlocked { failed })
        }
    }
}
