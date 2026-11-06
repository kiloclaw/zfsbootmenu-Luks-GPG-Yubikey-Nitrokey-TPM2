//! Importing a pool read/write before a change, as the legacy runtime
//! does: ZFSBootMenu imports pools read-only, and a snapshot, a rollback, a
//! clone or a new default has the pool exported and imported read/write
//! first, unless its `org.zfsbootmenu:readonly` property or a degraded
//! health forbids it, or the user does not allow it while a hibernated
//! system may resume. The menu's imports of an imported pool, read/write,
//! read-only again or rewound to its checkpoint, are the same export and
//! import ([`reimport`]).

use std::path::Path;

use zbm_platform::interaction::Interaction;
use zbm_platform::{Context, Runner};
use zbm_state::State;
use zbm_types::code::Coded as _;
use zbm_types::{ImportPolicy, PoolName};
use zbm_zfs::{Stderr, ZfsText};

use crate::error::{PoolError, WritableError};
use crate::import::{Access, Import, Rewind, import_pool};
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

/// Whether `pool` is imported read-write: its `readonly` property is `off`.
pub fn is_writable(runner: &dyn Runner, pool: &PoolName) -> bool {
    zbm_trace::debug!("pool set to {pool}"; %pool);
    let Some(readonly) = ZfsText::new(runner).pool_value(pool, "readonly", Stderr::Hidden) else {
        return false;
    };
    zbm_trace::debug!("{pool} readonly property: {readonly}"; %pool, %readonly);
    readonly == "off"
}

/// Exports `pool` with `zpool export`.
fn export(runner: &dyn Runner, pool: &PoolName) -> Result<(), PoolError> {
    zbm_trace::debug!("pool set to {pool}"; %pool);
    let code = ZfsText::new(runner).export(pool);
    zbm_trace::debug!("{pool} export process return: {code}"; %pool, %code);
    if code == 0 {
        Ok(())
    } else {
        Err(PoolError::Export {
            pool: pool.to_string(),
            status: code,
        })
    }
}

/// Turns off the module parameters of [`PARAMETERS`] that exist.
fn disable_risky_writes(context: &Context<'_>) {
    let files = context.files();
    for (parameter, what) in PARAMETERS {
        let path = Path::new(parameter);
        if files.exists(path) {
            zbm_trace::debug!("disabling {what} on writeable pools"; %what);
            let _ = files.write(path, b"0");
        }
    }
}

/// Why `pool` may not be imported read/write, if anything forbids it: its
/// [`READONLY`] property, or a degraded health recorded at start-up.
///
/// # Errors
///
/// [`WritableError::PoolProperty`] when the property cannot be read.
fn prohibition(
    context: &Context<'_>,
    state: &State<'_>,
    pool: &PoolName,
) -> Result<Option<String>, WritableError> {
    let flag = ZfsText::new(context.runner())
        .pool_value(pool, READONLY, Stderr::Console)
        .ok_or_else(|| WritableError::PoolProperty {
            pool: pool.to_string(),
        })?;
    let pool = pool.as_str();
    zbm_trace::debug!("{pool} {readonly} property: {flag}"; %pool, readonly = %READONLY, %flag);
    let mut reasons = Vec::new();
    if flag != "-" && flag != "off" {
        zbm_trace::error!(
            "read-write operation for '{pool}' prohibited - {readonly} set to {flag}";
            %pool, readonly = %READONLY, %flag
        );
        reasons.push(format!("{READONLY} is {flag}"));
    }
    if state.is_degraded(pool) {
        zbm_trace::debug!("prohibited: {pool} is marked as degraded"; %pool);
        zbm_trace::error!(
            "read-write operation for '{pool}' prohibited - pool marked as degraded";
            %pool
        );
        reasons.push("it is degraded".to_owned());
    }
    Ok((!reasons.is_empty()).then(|| reasons.join("; ")))
}

/// Makes `pool` writable: does nothing when it is imported read/write
/// already; otherwise, unless a property or its health forbids it, asks
/// the resume question through `interaction` where a suspend partition
/// exists, then exports the pool and imports it read/write with `policy`.
/// When that import fails, the pool is imported read-only again, as it was.
///
/// # Errors
///
/// [`WritableError::ReadWriteProhibited`], [`WritableError::PoolProperty`],
/// [`WritableError::ResumeRefused`], or [`WritableError::Pool`] when the
/// export or the import fails: the pool stays read-only.
pub fn set_rw_pool(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    state: &State<'_>,
    pool: &PoolName,
    policy: ImportPolicy,
) -> Result<(), WritableError> {
    let import = Import::pool(pool).policy(policy).access(Access::ReadWrite);
    reimport(context, interaction, state, pool, import)
}

/// Imports `pool`, which is imported, again as `import`, an import of it,
/// says: exports it, then imports it with the import's policy, access and
/// rewind, as the legacy runtime's read/write and read-only imports and
/// its rewind do, naming it by its identifier (its `guid`, read before the
/// export), so that a pool that is not imported and has its name is left
/// alone; by the import's own identifier, or its name, when the `guid`
/// cannot be read. Nothing
/// is done when the pool has the access asked already and nothing is
/// rewound; a rewind exports it whatever its access. Before the export, a
/// read/write import is refused by the [`READONLY`] property or a degraded
/// health, and asks the resume question. When the import fails, the pool
/// is imported read-only again, without a rewind, so that it is not left
/// exported.
///
/// # Errors
///
/// [`WritableError::ReadWriteProhibited`], [`WritableError::PoolProperty`]
/// or [`WritableError::ResumeRefused`] for a read/write import, before
/// anything is exported; [`WritableError::Pool`] when the export or the
/// import fails.
pub fn reimport(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    state: &State<'_>,
    pool: &PoolName,
    import: Import<'_>,
) -> Result<(), WritableError> {
    zbm_trace::debug!("pool set to {pool}"; %pool);
    let writes = import.is_read_write();
    if writes {
        disable_risky_writes(context);
    }
    if import.rewinds() == Rewind::No && is_writable(context.runner(), pool) == writes {
        return Ok(());
    }
    if writes {
        zbm_trace::debug!("{pool} is not already writable"; %pool);
        if let Some(reason) = prohibition(context, state, pool)? {
            return Err(WritableError::ReadWriteProhibited {
                pool: pool.to_string(),
                reason,
            });
        }
        let _choice = resume_prompt(context, interaction, state, pool)?;
    }
    let import = match imported_id(context.runner(), pool) {
        Some(id) => import.by_id(Some(id)),
        None => import,
    };
    export(context.runner(), pool)?;
    let imported = import_pool(context, state, import);
    zbm_trace::debug!("import_pool: {status}"; status = %i32::from(imported.is_err()));
    if imported.is_err() {
        zbm_trace::warn!(
            "{pool} could not be imported as asked; importing it read-only again";
            %pool
        );
        let again = import.access(Access::ReadOnly).rewind(Rewind::No);
        if let Err(error) = import_pool(context, state, again) {
            zbm_trace::error!(
                "{pool} could not be imported again: {error}";
                %pool, %error, error_code = %error.code()
            );
        }
    }
    Ok(imported?)
}

/// The numeric identifier of `pool`, which is imported: its `guid`.
fn imported_id(runner: &dyn Runner, pool: &PoolName) -> Option<u64> {
    ZfsText::new(runner)
        .pool_value(pool, "guid", Stderr::Hidden)?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests;
