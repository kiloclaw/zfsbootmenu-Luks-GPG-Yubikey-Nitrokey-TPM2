//! Closing volumes before booting: `luks_pools_on_device` and
//! `luks_close_all`.
//!
//! Pools must be exported before the mappings under them close; closing a
//! mapping makes the kernel wipe its volume key before the boot environment's
//! kernel starts.

use std::path::Path;

use zbm_core::exec::Stream;
use zbm_core::shell::{command_substitution, read_lines};

use crate::device::is_block_device;
use crate::mappings::Mapping;
use crate::session::Session;

/// Whether `zpool status -L -P` output names `target` or a partition of it
/// (`/dev/dm-3` or `/dev/dm-3p1`) as a vdev.
pub fn status_names(status: &str, target: &str) -> bool {
    status.split_whitespace().any(|field| {
        field == target
            || field.strip_prefix(target).is_some_and(|rest| {
                rest.strip_prefix('p').is_some_and(|digits| {
                    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
                })
            })
    })
}

impl Session<'_> {
    /// The imported pools with a vdev on `device` or one of its partitions
    /// (`luks_pools_on_device`).
    pub fn pools_on_device(&self, device: &str) -> Vec<String> {
        let system = self.system();
        let Ok(target) = system.files.canonicalize(Path::new(device)) else {
            return Vec::new();
        };
        let target = target.to_string_lossy().into_owned();
        let list = self
            .command("zpool")
            .args(["list", "-H", "-o", "name"])
            .stderr(Stream::Null);
        let pools = system
            .runner
            .run(&list)
            .map(|output| output.stdout_text())
            .unwrap_or_default();
        read_lines(&format!("{}\n", command_substitution(&pools)))
            .into_iter()
            .filter(|pool| !pool.is_empty())
            .filter(|pool| {
                let status = self
                    .command("zpool")
                    .args(["status", "-L", "-P", pool])
                    .stderr(Stream::Null);
                system
                    .runner
                    .run(&status)
                    .is_ok_and(|output| status_names(&output.stdout_text(), &target))
            })
            .collect()
    }

    /// Exports the pools on, and closes, the mappings ZFSBootMenu opened: all
    /// of them, or only `only` (`luks_close_all`). Mappings that cannot be
    /// closed stay recorded. Returns whether every mapping asked for closed.
    pub fn close_all(&mut self, only: Option<&str>) -> bool {
        let Some(mappings) = self.read_mappings() else {
            return true;
        };
        if !self.available() {
            return true;
        }
        let mut keep: Vec<Mapping> = Vec::new();
        let mut closed_all = true;
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
            for pool in self.pools_on_device(&path) {
                zbm_trace::debug!("exporting {pool} before closing {name}");
                let export = self
                    .command("zpool")
                    .args(["export", &pool])
                    .stdout(Stream::Null)
                    .stderr(Stream::Null);
                let exported = self
                    .system()
                    .runner
                    .run(&export)
                    .is_ok_and(|output| output.status.success());
                if !exported {
                    zbm_trace::warn!("unable to export {pool}, which uses {name}");
                }
            }
            if self.cryptsetup_close(&name) {
                zbm_trace::info!("closed LUKS mapping {name}");
            } else {
                zbm_trace::warn!("unable to close LUKS mapping {name}");
                keep.push(mapping);
                closed_all = false;
            }
        }
        self.write_mappings(&keep);
        closed_all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdevs_on_mappings_and_their_partitions_are_found() {
        let status = "  pool: tank\n state: ONLINE\nconfig:\n\n\tNAME          STATE\n\
                      \ttank          ONLINE\n\t  mirror-0    ONLINE\n\
                      \t    /dev/dm-3p1  ONLINE 0 0 0\n\t    /dev/vda2  ONLINE 0 0 0\n";
        assert!(status_names(status, "/dev/dm-3"));
        assert!(status_names(status, "/dev/vda2"));
        assert!(!status_names(status, "/dev/dm-"));
        assert!(!status_names(status, "/dev/vda"));
        assert!(!status_names("\t/dev/dm-3p\n", "/dev/dm-3"));
        assert!(!status_names("\t/dev/dm-3p1x\n", "/dev/dm-3"));
    }
}
