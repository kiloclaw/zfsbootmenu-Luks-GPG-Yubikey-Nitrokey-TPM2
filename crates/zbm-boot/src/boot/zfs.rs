//! What booting asks ZFS: dataset properties, encryption roots and their keys,
//! whether a pool is writable; and the export of pools before the new kernel
//! runs.

use zbm_platform::Runner;
use zbm_zfs::{Stderr, ZfsCommand, ZpoolCommand};

use crate::error::BootError;

/// The lines `while read -r line ... done <<< "$( command )"` reads.
fn here_lines(output: &str) -> Vec<String> {
    output
        .trim_end_matches('\n')
        .split('\n')
        .map(|line| line.trim_matches([' ', '\t']).to_owned())
        .collect()
}

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

/// `zfs get -H -o value <property> <dataset> 2>/dev/null`, or `None` when it
/// fails.
pub(crate) fn zfs_value(runner: &dyn Runner, property: &str, dataset: &str) -> Option<String> {
    let (ok, value) = substituted(ZfsCommand::new(runner).text(
        &["get", "-H", "-o", "value", property, dataset],
        Stderr::Hidden,
    ));
    ok.then_some(value)
}

/// The encryption root of `fs` (a snapshot counts as its filesystem), as
/// `be_has_encroot` finds it.
pub fn encryption_root(runner: &dyn Runner, fs: &str) -> Option<String> {
    // ${1%@*}
    let fs = fs.rsplit_once('@').map_or(fs, |(fs, _)| fs);
    if fs.is_empty() {
        zbm_trace::error!("fs is undefined");
        return None;
    }
    zbm_trace::debug!("fs set to {fs}");
    let pool = fs.split('/').next().unwrap_or(fs);
    let feature = ZpoolCommand::new(runner).text(
        &["list", "-H", "-o", "feature@encryption", pool],
        Stderr::Hidden,
    );
    if substituted(feature).1 != "active" {
        zbm_trace::debug!("feature@encryption not active on {pool}");
        return None;
    }
    let encroot = zfs_value(runner, "encryptionroot", fs)?;
    zbm_trace::debug!("{fs} encryptionroot property: {encroot}");
    (encroot != "-").then_some(encroot)
}

/// The encryption root of `fs` if its key is not loaded, as `be_is_locked`
/// decides.
pub fn locked_root(runner: &dyn Runner, fs: &str) -> Option<String> {
    if fs.is_empty() {
        zbm_trace::error!("fs is undefined");
        return None;
    }
    zbm_trace::debug!("fs set to {fs}");
    let encroot = encryption_root(runner, fs)?;
    zbm_trace::debug!("{encroot} discovered as encryption root for {fs}");
    let keystatus = ZfsCommand::new(runner).text(
        &["get", "-H", "-o", "value", "keystatus", &encroot],
        Stderr::Merged,
    );
    let keystatus = substituted(keystatus).1;
    zbm_trace::debug!("{encroot} keystatus: {keystatus}");
    (keystatus == "unavailable").then_some(encroot)
}

/// Whether `pool` is imported read-write (`is_writable`).
pub fn is_writable(runner: &dyn Runner, pool: &str) -> bool {
    if pool.is_empty() {
        zbm_trace::error!("pool is undefined");
        return false;
    }
    zbm_trace::debug!("pool set to {pool}");
    let (ok, readonly) = substituted(ZpoolCommand::new(runner).text(
        &["get", "-H", "-o", "value", "readonly", pool],
        Stderr::Hidden,
    ));
    if !ok {
        return false;
    }
    zbm_trace::debug!("{pool} readonly property: {readonly}");
    readonly == "off"
}

/// Exports `pool` (`export_pool`).
///
/// # Errors
///
/// [`BootError::NoPool`] without a pool, [`BootError::Export`] when
/// `zpool export` fails.
pub fn export_pool(runner: &dyn Runner, pool: &str) -> Result<(), BootError> {
    if pool.is_empty() {
        zbm_trace::error!("pool is undefined");
        return Err(BootError::NoPool);
    }
    zbm_trace::debug!("pool set to {pool}");
    let code = match ZpoolCommand::new(runner).text(&["export", pool], Stderr::Console) {
        Ok(output) => output.status.shell_code(),
        Err(_) => 126,
    };
    zbm_trace::debug!("{pool} export process return: {code}");
    if code == 0 {
        Ok(())
    } else {
        Err(BootError::Export {
            pool: pool.to_owned(),
            status: code,
        })
    }
}

/// Exports every imported pool that is writable, ignoring failures, so that
/// the kernel about to run finds them cleanly exported.
pub(super) fn export_writable_pools(runner: &dyn Runner) {
    let pools = ZpoolCommand::new(runner).text(&["list", "-H", "-o", "name"], Stderr::Console);
    for pool in here_lines(&substituted(pools).1) {
        if is_writable(runner, &pool) {
            zbm_trace::debug!("{pool} is read/write, exporting");
            let _ = export_pool(runner, &pool);
        }
    }
}
