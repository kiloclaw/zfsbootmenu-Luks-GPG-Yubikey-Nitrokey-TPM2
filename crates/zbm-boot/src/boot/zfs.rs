//! What booting asks ZFS: dataset properties, encryption roots and their keys,
//! whether a pool is writable; and the export of pools before the new kernel
//! runs.

use zbm_platform::Runner;
use zbm_types::{DatasetName, PoolName};
use zbm_zfs::{Stderr, ZfsName, ZfsText};

use crate::error::BootError;

/// What `$( command )` gives for the run `result`, and whether it succeeded.
pub(super) fn substituted(result: std::io::Result<zbm_platform::exec::Output>) -> (bool, String) {
    match result {
        Ok(output) => (
            output.status.is_success(),
            output.stdout_text().trim_end_matches('\n').to_owned(),
        ),
        Err(error) => (false, error.to_string()),
    }
}

/// The value of `property` of `name`, quietly; `None` when `zfs` fails.
pub(crate) fn zfs_value(
    runner: &dyn Runner,
    property: &str,
    name: &impl ZfsName,
) -> Option<String> {
    ZfsText::new(runner).value(name, property, Stderr::Hidden)
}

/// The encryption root of `fs`, or `None` when its pool does not use
/// encryption or `fs` is not encrypted. (A snapshot's is its dataset's.)
pub fn encryption_root(runner: &dyn Runner, fs: &DatasetName) -> Option<DatasetName> {
    zbm_trace::debug!("fs set to {fs}"; %fs);
    let pool = fs.pool();
    let feature =
        ZfsText::new(runner).pool_list(Some(&pool), &["feature@encryption"], Stderr::Hidden);
    if feature.trim_end_matches('\n') != "active" {
        zbm_trace::debug!("feature@encryption not active on {pool}"; %pool);
        return None;
    }
    let encroot = zfs_value(runner, "encryptionroot", fs)?;
    zbm_trace::debug!("{fs} encryptionroot property: {encroot}"; %fs, %encroot);
    encroot.parse().ok()
}

/// The encryption root of `fs` if its key is not loaded: its `keystatus` is
/// `unavailable`.
pub fn locked_root(runner: &dyn Runner, fs: &DatasetName) -> Option<DatasetName> {
    zbm_trace::debug!("fs set to {fs}"; %fs);
    let encroot = encryption_root(runner, fs)?;
    zbm_trace::debug!("{encroot} discovered as encryption root for {fs}"; %encroot, %fs);
    let keystatus = ZfsText::new(runner)
        .value(&encroot, "keystatus", Stderr::Merged)
        .unwrap_or_default();
    zbm_trace::debug!("{encroot} keystatus: {keystatus}"; %encroot, %keystatus);
    (keystatus == "unavailable").then_some(encroot)
}

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
///
/// # Errors
///
/// [`BootError::Export`] when `zpool export` fails.
pub fn export_pool(runner: &dyn Runner, pool: &PoolName) -> Result<(), BootError> {
    zbm_trace::debug!("pool set to {pool}"; %pool);
    let code = ZfsText::new(runner).export(pool);
    zbm_trace::debug!("{pool} export process return: {code}"; %pool, %code);
    if code == 0 {
        Ok(())
    } else {
        Err(BootError::Export {
            pool: pool.to_string(),
            status: code,
        })
    }
}

/// Exports every imported pool that is writable, ignoring failures, so that
/// the kernel about to run finds them cleanly exported.
pub(super) fn export_writable_pools(runner: &dyn Runner) {
    for pool in ZfsText::new(runner).imported(Stderr::Console) {
        if is_writable(runner, &pool) {
            zbm_trace::debug!("{pool} is read/write, exporting"; %pool);
            let _ = export_pool(runner, &pool);
        }
    }
}
