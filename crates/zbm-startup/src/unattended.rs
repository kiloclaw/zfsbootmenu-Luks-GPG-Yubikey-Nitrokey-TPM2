//! After the machine unlocked itself (ROADMAP.md, SE10): every LUKS volume
//! of this boot opened by a key method that needs nobody, a TPM2 token
//! without a PIN, Clevis, a key file or a key script
//! (`zbm_state::State::unattended_mappings`). Nobody chose what the
//! machine then imports and boots, so start-up holds to the volumes that
//! unlocked: a pool of another host is not adopted, and only a pool on one
//! of those volumes boots by itself.

use std::path::Path;

use zbm_platform::System;
use zbm_state::State;
use zbm_tools::devices::is_on;
use zbm_types::{DatasetName, ImportPolicy};
use zbm_zfs::{Zfs as _, ZfsCli};

/// The import policy after the unlock: `hostid`, the default, which adopts
/// the host ID of a pool last imported elsewhere, imports as `strict` when
/// the volumes unlocked themselves; `force` and `strict`, which only an
/// option gives, stay.
pub fn import_policy(state: &State<'_>, policy: ImportPolicy) -> ImportPolicy {
    if policy == ImportPolicy::Hostid && state.unattended_mappings().is_some() {
        zbm_trace::info!(
            "the volumes unlocked themselves: pools import with this machine's host ID alone"
        );
        ImportPolicy::Strict
    } else {
        policy
    }
}

/// Whether `bootfs` may boot with nobody choosing it, by the countdown or
/// `zbm.skip`: always, unless the volumes unlocked themselves; then only a
/// boot environment on a pool with a vdev on one of them, not one a disk
/// plugged in brought.
pub fn may_boot_by_itself(system: &System<'_>, state: &State<'_>, bootfs: &DatasetName) -> bool {
    let Some(mappings) = state.unattended_mappings() else {
        return true;
    };
    let pool = bootfs.pool();
    let targets: Vec<String> = mappings
        .iter()
        .filter_map(|name| {
            let path = Path::new("/dev/mapper").join(name);
            system.files.canonicalize(&path).ok()
        })
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let devices = ZfsCli::new(system.runner)
        .devices(&pool)
        .unwrap_or_default();
    let on_unlocked = devices.iter().any(|device| {
        let device = system.files.canonicalize(Path::new(device)).map_or_else(
            |_| device.clone(),
            |path| path.to_string_lossy().into_owned(),
        );
        targets.iter().any(|target| is_on(&device, target))
    });
    if !on_unlocked {
        zbm_trace::warn!(
            "{bootfs} is on no volume that unlocked itself: it boots only when chosen";
            %bootfs
        );
    }
    on_unlocked
}

#[cfg(test)]
mod tests;
