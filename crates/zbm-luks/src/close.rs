//! Closing volumes before booting: finding the pools on a mapping, then
//! exporting them and closing the mappings ZFSBootMenu opened.
//!
//! Pools must be exported before the mappings under them close; closing a
//! mapping makes the kernel wipe its volume key before the boot environment's
//! kernel starts.

use std::path::Path;

use zbm_types::MappingName;
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
    /// The imported pools with a vdev on `device` or one of its partitions.
    pub fn pools_on_device(&self, device: &str) -> Vec<String> {
        let context = *self.context();
        let Ok(target) = context.files().canonicalize(Path::new(device)) else {
            return Vec::new();
        };
        let target = target.to_string_lossy().into_owned();
        let zfs = ZfsCli::new(context.runner());
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
    /// of them, or only `only`. Mappings that cannot be closed stay
    /// recorded, and so do those another session (`zluks open`) recorded
    /// meanwhile: only the mappings that are gone are taken out of the
    /// record, under its lock.
    ///
    /// # Errors
    ///
    /// [`LuksError::NotClosed`] with the mappings that stay open.
    pub fn close_all(&mut self, only: Option<&MappingName>) -> Result<(), LuksError> {
        let Some(mappings) = self.mappings().read() else {
            return Ok(());
        };
        if !self.available() {
            return Ok(());
        }
        let mut gone: Vec<String> = Vec::new();
        let mut open: Vec<String> = Vec::new();
        for Mapping { name, .. } in mappings {
            if only.is_some_and(|only| only.as_str() != name) {
                continue;
            }
            let path = self
                .settings()
                .mapper
                .join(&name)
                .to_string_lossy()
                .into_owned();
            if !is_block_device(self.context().files(), &path) {
                gone.push(name);
                continue;
            }
            let zfs = ZfsCli::new(self.context().runner());
            for pool in self.pools_on_device(&path) {
                zbm_trace::debug!("exporting {pool} before closing {name}"; %pool, %name);
                let exported = pool.parse().is_ok_and(|pool| zfs.export(&pool).is_ok());
                if !exported {
                    zbm_trace::warn!("unable to export {pool}, which uses {name}"; %pool, %name);
                }
            }
            let closed = name
                .parse::<MappingName>()
                .is_ok_and(|mapping| self.cryptsetup_close(&mapping).is_ok());
            if closed {
                zbm_trace::info!("closed LUKS mapping {name}"; %name);
                gone.push(name);
            } else {
                zbm_trace::warn!("unable to close LUKS mapping {name}"; %name);
                open.push(name);
            }
        }
        self.mappings().remove(&gone);
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
