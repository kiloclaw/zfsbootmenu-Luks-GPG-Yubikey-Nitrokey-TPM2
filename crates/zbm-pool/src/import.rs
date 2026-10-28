//! Importing pools: `zpool import` with the options of an [`Import`], host ID
//! matching for pools that another host used last, and what `zpool` says
//! about the pools it cannot import.
//!
//! The command lines and log messages are those of the legacy runtime,
//! which the parity tests of `zbm-boot`'s start-up stage compare.

use zbm_platform::exec::Output;
use zbm_platform::{Context, Runner};
use zbm_state::State;
use zbm_types::{ImportPolicy, PoolName};
use zbm_zfs::{Stderr, ZpoolCommand};

use crate::error::PoolError;
use crate::hostid::{self, HostId};
use crate::settings::{Build, ImportSettings};
use crate::text::here_lines;

/// Which pools an import imports.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Target<'a> {
    /// Every pool that can be imported (`zpool import -a`).
    #[default]
    All,
    /// One pool.
    Pool(&'a PoolName),
}

/// Whether imported pools can be written.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Access {
    /// Read-only (`-o readonly=on`), as every import of the start-up is.
    #[default]
    ReadOnly,
    /// Read-write (`-o readonly=off`).
    ReadWrite,
}

/// Whether to rewind a pool to its checkpoint.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Rewind {
    /// No.
    #[default]
    No,
    /// Yes (`--rewind-to-checkpoint`); only for one pool.
    ToCheckpoint,
}

/// How `zpool import` imports, without mounting (`-N`).
///
/// ```
/// use zbm_pool::import::{Access, Import};
/// use zbm_types::PoolName;
/// use zbm_types::ImportPolicy;
///
/// let tank = "tank".parse::<PoolName>().unwrap();
/// let import = Import::pool(&tank)
///     .policy(ImportPolicy::Force)
///     .access(Access::ReadWrite);
/// let args = ["import", "-N", "-f", "-o", "readonly=off", "tank"];
/// assert_eq!(import.arguments(), args);
/// let all = ["import", "-N", "-o", "readonly=on", "-a"];
/// assert_eq!(Import::all().arguments(), all);
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Import<'a> {
    target: Target<'a>,
    policy: ImportPolicy,
    access: Access,
    rewind: Rewind,
}

impl<'a> Import<'a> {
    /// An import of every pool, read-only, by this host's ID.
    pub fn all() -> Import<'a> {
        Import::default()
    }

    /// An import of `pool`, read-only, by this host's ID.
    pub fn pool(pool: &'a PoolName) -> Import<'a> {
        Import {
            target: Target::Pool(pool),
            ..Import::default()
        }
    }

    /// An import of `target`: one pool, or every pool when `None`.
    pub fn target(target: Option<&'a PoolName>) -> Import<'a> {
        target.map_or_else(Import::all, Import::pool)
    }

    /// The same import under `policy`: [`ImportPolicy::Force`] imports pools
    /// in use elsewhere (`-f`).
    #[must_use]
    pub fn policy(mut self, policy: ImportPolicy) -> Import<'a> {
        self.policy = policy;
        self
    }

    /// The same import with `access`.
    #[must_use]
    pub fn access(mut self, access: Access) -> Import<'a> {
        self.access = access;
        self
    }

    /// The same import with `rewind`.
    #[must_use]
    pub fn rewind(mut self, rewind: Rewind) -> Import<'a> {
        self.rewind = rewind;
        self
    }

    /// The options of `zpool import`.
    fn options(self) -> Vec<&'static str> {
        let mut args = vec!["-N"];
        if self.policy == ImportPolicy::Force {
            args.push("-f");
        }
        args.extend(match self.access {
            Access::ReadWrite => ["-o", "readonly=off"],
            Access::ReadOnly => ["-o", "readonly=on"],
        });
        if self.rewind == Rewind::ToCheckpoint {
            args.push("--rewind-to-checkpoint");
        }
        args
    }

    /// Logs the options, one debug message each.
    fn log_options(self) {
        if self.policy == ImportPolicy::Force {
            zbm_trace::debug!("import_policy set: force");
        }
        match self.access {
            Access::ReadWrite => zbm_trace::debug!("read_write set: yes"),
            Access::ReadOnly => zbm_trace::debug!("read_write unset"),
        }
        if self.rewind == Rewind::ToCheckpoint {
            zbm_trace::debug!("rewind_to_checkpoint set: yes");
        }
    }

    /// The pool argument: the pool, or `-a`.
    fn pool_argument(self) -> &'a str {
        match self.target {
            Target::Pool(pool) => pool.as_str(),
            Target::All => "-a",
        }
    }

    /// The arguments of `zpool` for this import.
    pub fn arguments(self) -> Vec<&'a str> {
        let mut args = vec!["import"];
        args.extend(self.options());
        args.push(self.pool_argument());
        args
    }
}

/// The standard output of a run, empty when the program could not start.
pub(crate) fn stdout(run: std::io::Result<Output>) -> String {
    run.map(|output| output.stdout_text()).unwrap_or_default()
}

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
    match import.target {
        Target::Pool(pool) => zbm_trace::debug!("pool set to {pool}"),
        Target::All if import.rewind == Rewind::ToCheckpoint => {
            zbm_trace::error!("rewind only works on a specific pool");
            return Err(PoolError::RewindAll);
        }
        Target::All => zbm_trace::debug!("attempting to import all pools"),
    }
    import.log_options();
    let args = import.arguments();
    let shown = args.get(1..).unwrap_or_default().join(" ");
    zbm_trace::debug!("zpool import arguments: {shown}");
    let (code, output) = match ZpoolCommand::new(context.runner()).text(&args, Stderr::Merged) {
        Ok(output) => (output.status.shell_code(), output.stdout_text()),
        Err(error) => (126, error.to_string()),
    };
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

/// The pools that are imported, as `zpool list` names them.
pub fn imported_pools(runner: &dyn Runner) -> Vec<String> {
    let list = ZpoolCommand::new(runner).text(&["list", "-H", "-o", "name"], Stderr::Hidden);
    here_lines(&stdout(list))
        .into_iter()
        .filter(|pool| !pool.is_empty())
        .collect()
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
    let list = ZpoolCommand::new(context.runner()).text(&["import"], Stderr::Hidden);
    online_pools(&stdout(list), settings.prefer_pool.as_ref())
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

    for pool in importable {
        zbm_trace::debug!("trying to import: {pool}");
        let probe = ZpoolCommand::new(context.runner()).text(
            &["import", "-o", "readonly=on", "-N", pool.as_str()],
            Stderr::Merged,
        );
        let output = stdout(probe);
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
    let zpool = ZpoolCommand::new(runner);
    let list = zpool.text(&["import"], Stderr::Hidden);
    let mut id = String::new();
    for line in here_lines(&stdout(list)) {
        if line.starts_with("id") {
            line.strip_prefix("id: ")
                .unwrap_or(&line)
                .clone_into(&mut id);
        } else if line.starts_with("state") {
            if line.strip_prefix("state: ").unwrap_or(&line) == "UNAVAIL" {
                let import = zpool.text(&["import", "-N", &id], Stderr::Merged);
                for error in here_lines(&stdout(import)) {
                    zbm_trace::error!("{}", error);
                }
            }
            id.clear();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
