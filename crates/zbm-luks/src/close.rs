//! Closing volumes before booting: `luks_pools_on_device` and
//! `luks_close_all`.
//!
//! Pools must be exported before the mappings under them close; closing a
//! mapping makes the kernel wipe its volume key before the boot environment's
//! kernel starts.

use std::path::Path;

use zbm_zfs::{Zfs, ZfsCli};

use crate::device::is_block_device;
use crate::error::LuksError;
use crate::mappings::Mapping;
use crate::unlocker::Unlocker;

/// Whether the vdev `device` is `target` or a partition of it: `/dev/dm-3`
/// or `/dev/dm-3p1` for `/dev/dm-3`.
pub fn is_on(device: &str, target: &str) -> bool {
    device == target
        || device.strip_prefix(target).is_some_and(|rest| {
            rest.strip_prefix('p').is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
            })
        })
}

impl Unlocker<'_> {
    /// The imported pools with a vdev on `device` or one of its partitions
    /// (`luks_pools_on_device`).
    pub fn pools_on_device(&self, device: &str) -> Vec<String> {
        let system = self.system();
        let Ok(target) = system.files.canonicalize(Path::new(device)) else {
            return Vec::new();
        };
        let target = target.to_string_lossy().into_owned();
        let zfs = ZfsCli::new(system.runner);
        zfs.pool_names()
            .unwrap_or_default()
            .into_iter()
            .filter(|pool| {
                zfs.devices(pool)
                    .is_ok_and(|devices| devices.iter().any(|device| is_on(device, &target)))
            })
            .map(|pool| pool.to_string())
            .collect()
    }

    /// Exports the pools on, and closes, the mappings ZFSBootMenu opened: all
    /// of them, or only `only` (`luks_close_all`). Mappings that cannot be
    /// closed stay recorded.
    ///
    /// # Errors
    ///
    /// [`LuksError::NotClosed`] with the mappings that stay open.
    pub fn close_all(&mut self, only: Option<&str>) -> Result<(), LuksError> {
        let Some(mappings) = self.mappings().read() else {
            return Ok(());
        };
        if !self.available() {
            return Ok(());
        }
        let mut keep: Vec<Mapping> = Vec::new();
        let mut open: Vec<String> = Vec::new();
        for mapping in mappings {
            let name = mapping.name.clone();
            if only.is_some_and(|only| only != name) {
                keep.push(mapping);
                continue;
            }
            let path = self
                .settings()
                .mapper
                .join(&name)
                .to_string_lossy()
                .into_owned();
            if !is_block_device(self.system().files, &path) {
                continue;
            }
            let zfs = ZfsCli::new(self.system().runner);
            for pool in self.pools_on_device(&path) {
                zbm_trace::debug!("exporting {pool} before closing {name}");
                let exported = pool.parse().is_ok_and(|pool| zfs.export(&pool).is_ok());
                if !exported {
                    zbm_trace::warn!("unable to export {pool}, which uses {name}");
                }
            }
            if self.cryptsetup_close(&name).is_ok() {
                zbm_trace::info!("closed LUKS mapping {name}");
            } else {
                zbm_trace::warn!("unable to close LUKS mapping {name}");
                keep.push(mapping);
                open.push(name);
            }
        }
        self.mappings().write(&keep);
        if open.is_empty() {
            Ok(())
        } else {
            Err(LuksError::NotClosed { names: open })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdevs_on_mappings_and_their_partitions_are_found() {
        assert!(is_on("/dev/dm-3p1", "/dev/dm-3"));
        assert!(is_on("/dev/vda2", "/dev/vda2"));
        assert!(!is_on("/dev/dm-3p1", "/dev/dm-"));
        assert!(!is_on("/dev/vda2", "/dev/vda"));
        assert!(!is_on("/dev/dm-3p", "/dev/dm-3"));
        assert!(!is_on("/dev/dm-3p1x", "/dev/dm-3"));
    }
}
