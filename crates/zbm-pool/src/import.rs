//! Importing pools: `zpool import` with the options of an [`Import`], host ID
//! matching for pools that another host used last, and what `zpool` says
//! about the pools it cannot import.
//!
//! The command lines (built by `zbm-zfs`, which [`Import`] comes from) and
//! log messages are those of the legacy runtime, which the parity tests of
//! `zbm-boot`'s start-up stage compare.

use zbm_platform::{Context, Runner};
use zbm_state::State;
use zbm_types::PoolName;
use zbm_zfs::{Stderr, ZfsText};

use crate::error::PoolError;
use crate::hostid::{self, HostId};
use crate::settings::{Build, ImportSettings};
use crate::text::here_lines;

pub use zbm_zfs::import::{Access, Import, Rewind, Target};

/// Imports pools without mounting them; on failure, logs the host ID ZFS
/// used and what `zpool` said.
///
/// # Errors
///
/// [`PoolError::RewindAll`] for a rewind without a pool,
/// [`PoolError::Import`] when `zpool import` fails.
pub fn import_pool(
    context: &Context<'_>,
    state: &State<'_>,
    import: Import<'_>,
) -> Result<(), PoolError> {
    match import.imports() {
        Target::Pool(pool) => zbm_trace::debug!("pool set to {pool}"),
        Target::All if import.rewinds() == Rewind::ToCheckpoint => {
            zbm_trace::error!("rewind only works on a specific pool");
            return Err(PoolError::RewindAll);
        }
        Target::All => zbm_trace::debug!("attempting to import all pools"),
    }
    import.log_options();
    let args = import.arguments();
    let shown = args.get(1..).unwrap_or_default().join(" ");
    zbm_trace::debug!("zpool import arguments: {shown}");
    let (code, output) = ZfsText::new(context.runner()).import(import);
    if code == 0 {
        zbm_trace::debug!("successful pool import");
        return Ok(());
    }
    let mut messages = Vec::new();
    let apparent = hostid::apparent_hostid(context.files(), state, &mut messages);
    for message in &messages {
        message.emit();
    }
    zbm_trace::debug!(
        "import process failed with code {code}, apparent hostid {}",
        apparent.as_deref().unwrap_or("unknown")
    );
    zbm_trace::debug!("{}", output.trim_end_matches('\n'));
    Err(PoolError::Import {
        target: import.pool_argument().to_owned(),
        status: code,
    })
}

/// The pools that are imported.
pub fn imported_pools(runner: &dyn Runner) -> Vec<PoolName> {
    ZfsText::new(runner).imported(Stderr::Hidden)
}

/// The importable pools `zpool import` lists as `ONLINE`, leaving out the
/// preferred pool: the pools host ID matching tries.
pub fn online_pools(output: &str, prefer: Option<&PoolName>) -> Vec<String> {
    let preferred = prefer.map_or("", PoolName::as_str);
    let mut pools = Vec::new();
    let mut pool = String::new();
    for line in here_lines(output) {
        if line.starts_with("pool") {
            line.strip_prefix("pool: ")
                .unwrap_or(&line)
                .clone_into(&mut pool);
        } else if line.starts_with("state") {
            let state = line.strip_prefix("state: ").unwrap_or(&line);
            if state == "ONLINE" && !pool.is_empty() && pool != preferred {
                pools.push(std::mem::take(&mut pool));
            }
        }
    }
    pools
}

/// A pool that host ID matching imported, and the host ID it used.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostidMatch {
    /// The pool.
    pub pool: PoolName,
    /// The host ID written to `/etc/hostid`.
    pub hostid: HostId,
}

/// The pools host ID matching tries: `single`, or else each `ONLINE` pool
/// that is not the preferred one.
fn candidates(
    context: &Context<'_>,
    settings: &ImportSettings,
    single: Option<&PoolName>,
) -> Vec<PoolName> {
    if let Some(pool) = single {
        return vec![pool.clone()];
    }
    let list = ZfsText::new(context.runner()).importable();
    online_pools(&list, settings.prefer_pool.as_ref())
        .iter()
        .filter_map(|pool| pool.parse::<PoolName>().ok())
        .collect()
}

/// Imports a pool that belongs to another host with that host's ID:
/// `single`, or else each `ONLINE` pool that is not the
/// preferred one, until one is imported.
pub fn match_hostid(
    context: &Context<'_>,
    state: &State<'_>,
    settings: &ImportSettings,
    single: Option<&PoolName>,
) -> Option<HostidMatch> {
    let importable = candidates(context, settings, single);
    let names: Vec<&str> = importable.iter().map(PoolName::as_str).collect();
    zbm_trace::debug!("importable pools: {}", names.join(" "));

    let zpool = ZfsText::new(context.runner());
    for pool in importable {
        zbm_trace::debug!("trying to import: {pool}");
        let output = zpool.probe_import(&pool);
        let Some(hostid) = HostId::scrape(output.trim_end_matches('\n')) else {
            zbm_trace::debug!("unable to scrape hostid for {pool}, skipping");
            continue;
        };
        zbm_trace::debug!("discovered pool owner hostid: {hostid}");
        if hostid::write(context.files(), &hostid).is_err() {
            zbm_trace::debug!("failed to set hostid {hostid}, skipping import of pool {pool}");
            continue;
        }
        let import = Import::pool(&pool).policy(settings.policy);
        if import_pool(context, state, import).is_ok() {
            zbm_trace::debug!("successfully imported {pool}");
            if settings.build == Build::Local {
                zbm_trace::warn!("imported {pool} with assumed hostid {hostid}");
                zbm_trace::warn!(
                    "set spl_hostid={hostid} on ZBM KCL or regenerate with corrected /etc/hostid"
                );
            }
            return Some(HostidMatch { pool, hostid });
        }
    }
    None
}

/// Logs why each `UNAVAIL` pool cannot be imported, in the words of
/// `zpool import`.
pub fn log_unimportable(runner: &dyn Runner) {
    let zpool = ZfsText::new(runner);
    let mut id = String::new();
    for line in here_lines(&zpool.importable()) {
        if line.starts_with("id") {
            line.strip_prefix("id: ")
                .unwrap_or(&line)
                .clone_into(&mut id);
        } else if line.starts_with("state") {
            if line.strip_prefix("state: ").unwrap_or(&line) == "UNAVAIL" {
                for error in here_lines(&zpool.probe_import_id(&id)) {
                    zbm_trace::error!("{}", error);
                }
            }
            id.clear();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
