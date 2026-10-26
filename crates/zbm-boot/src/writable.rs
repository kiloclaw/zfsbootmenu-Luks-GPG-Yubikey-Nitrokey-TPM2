//! Importing a pool read/write before a change, as the legacy runtime
//! does: ZFSBootMenu imports pools read-only, and a snapshot, a rollback, a
//! clone or a new default has the pool exported and imported read/write
//! first, unless its `org.zfsbootmenu:readonly` property or a degraded
//! health forbids it, or the user does not allow it while a hibernated
//! system may resume.

use std::path::Path;

use zbm_platform::System;
use zbm_pool::PoolRef;
use zbm_pool::import::{Access, Import, import_pool};
use zbm_state::State;
use zbm_types::ImportPolicy;
use zbm_zfs::ZpoolCommand;

use crate::boot::{export_pool, is_writable};
use crate::error::BootError;
use crate::resume::resume_prompt;

/// The module parameters set to 0 before a pool is written, as the legacy
/// runtime does: block cloning, and `zfs_dmu_offset_next_sync`, which
/// corrupted copies with some versions.
const PARAMETERS: [(&str, &str); 2] = [
    (
        "/sys/module/zfs/parameters/zfs_bclone_enabled",
        "block cloning",
    ),
    (
        "/sys/module/zfs/parameters/zfs_dmu_offset_next_sync",
        "zfs_dmu_offset_next_sync",
    ),
];

/// The pool property that forbids read/write imports.
pub const READONLY: &str = "org.zfsbootmenu:readonly";

/// Turns off the module parameters of [`PARAMETERS`] that exist.
fn disable_risky_writes(system: &System<'_>) {
    for (parameter, what) in PARAMETERS {
        let path = Path::new(parameter);
        if system.files.exists(path) {
            zbm_trace::debug!("disabling {what} on writeable pools");
            let _ = system.files.write(path, b"0");
        }
    }
}

/// Why `pool` may not be imported read/write, if anything forbids it: its
/// [`READONLY`] property, or a degraded health recorded at start-up.
///
/// # Errors
///
/// [`BootError::PoolProperty`] when the property cannot be read.
fn prohibition(
    system: &System<'_>,
    state: &State<'_>,
    pool: &str,
) -> Result<Option<String>, BootError> {
    let flag = ZpoolCommand::new(system.runner)
        .get_value(READONLY, pool)
        .ok_or_else(|| BootError::PoolProperty {
            pool: pool.to_owned(),
        })?;
    zbm_trace::debug!("{pool} {READONLY} property: {flag}");
    let mut reasons = Vec::new();
    if flag != "-" && flag != "off" {
        zbm_trace::error!(
            "read-write operation for '{pool}' prohibited - {READONLY} set to {flag}"
        );
        reasons.push(format!("{READONLY} is {flag}"));
    }
    if state.is_degraded(pool) {
        zbm_trace::debug!("prohibited: {pool} is marked as degraded");
        zbm_trace::error!("read-write operation for '{pool}' prohibited - pool marked as degraded");
        reasons.push("it is degraded".to_owned());
    }
    Ok((!reasons.is_empty()).then(|| reasons.join("; ")))
}

/// Makes `pool` writable: does nothing when it is imported read/write
/// already; otherwise, unless a property or its health forbids it, asks
/// about hibernation where a suspend partition exists, then exports the
/// pool and imports it read/write with `policy`.
///
/// # Errors
///
/// [`BootError::ReadWriteProhibited`], [`BootError::PoolProperty`],
/// [`BootError::ResumeRefused`], [`BootError::Export`] or
/// [`BootError::Pool`] when the pool stays read-only.
pub fn set_rw_pool(
    system: &System<'_>,
    state: &State<'_>,
    pool: &str,
    policy: ImportPolicy,
) -> Result<(), BootError> {
    let Some(reference) = PoolRef::new(pool) else {
        zbm_trace::error!("pool is undefined");
        return Err(BootError::NoPool);
    };
    zbm_trace::debug!("pool set to {pool}");
    disable_risky_writes(system);
    if is_writable(system.runner, pool) {
        return Ok(());
    }
    zbm_trace::debug!("{pool} is not already writable");
    if let Some(reason) = prohibition(system, state, pool)? {
        return Err(BootError::ReadWriteProhibited {
            pool: pool.to_owned(),
            reason,
        });
    }
    let _choice = resume_prompt(system, state, pool)?;
    export_pool(system.runner, pool)?;
    let import = Import::pool(&reference)
        .policy(policy)
        .access(Access::ReadWrite);
    let imported = import_pool(system, state, import);
    zbm_trace::debug!("import_pool: {}", i32::from(imported.is_err()));
    Ok(imported?)
}

#[cfg(test)]
mod tests;
