//! The export of pools before the new kernel runs, and what a program's run
//! gives as the shell's `$( command )`.

use zbm_platform::Runner;
use zbm_types::PoolName;
use zbm_zfs::{Stderr, ZfsText};

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

/// Whether `pool` is imported read-write: its `readonly` property is `off`.
pub(super) fn is_writable(runner: &dyn Runner, pool: &PoolName) -> bool {
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
