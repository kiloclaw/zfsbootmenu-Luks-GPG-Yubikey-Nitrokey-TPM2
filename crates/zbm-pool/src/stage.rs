//! The start-up stage that imports pools (`init.d/50-import-pools`): the
//! import loop of [`crate::retry`] run against the system, then the
//! restriction of unhealthy pools, the check for unsupported features and the
//! choice of the default boot environment.
//!
//! What needs a person at the console (the retry prompt, the warning about
//! unsupported features, the recovery shell) and the late unlocking of LUKS
//! volumes go through the [`Operator`] port, which the start-up program
//! provides.

use zbm_config::spec::properties::BOOTFS;
use zbm_platform::{Runner, System};
use zbm_state::State;
use zbm_trace::Level;
use zbm_types::{Answer, DatasetName};
use zbm_zfs::{Stderr, ZfsCommand, ZpoolCommand};

use crate::hostid::HostId;
use crate::import::{Import, import_pool, imported_pools, log_unimportable, match_hostid, stdout};
use crate::retry::{AfterImport, Found, ImportLoop, Matched, Next};
use crate::settings::{BootfsPreference, Build, ImportSettings, PoolRef, Requirement, RetryDelay};
use crate::text::{here_lines, tab_fields, tab_lines};

/// Whether late unlocking opened a LUKS volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Late {
    /// At least one volume that appeared late was opened.
    Unlocked,
    /// None was.
    Nothing,
}

/// What the import stage needs from the program that runs it.
pub trait Operator {
    /// Unlocks LUKS volumes on disks that appeared since the last unlock.
    fn unlock_late(&mut self) -> Late;

    /// Says that `target` (every pool when `None`) cannot be imported and
    /// counts down `delay`: [`Answer::Continue`] retries, [`Answer::Escape`]
    /// opens the recovery shell.
    fn retry(&mut self, target: Option<&PoolRef>, delay: RetryDelay) -> Answer;

    /// Warns that a pool has features this ZFS does not support, with what
    /// to do about it.
    fn unsupported_features(&mut self, advice: &str);

    /// Opens the recovery shell with `message`; the stage carries on when it
    /// ends.
    fn emergency_shell(&mut self, message: &str);
}

/// What the stage found.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Imported {
    /// The default boot environment (`BOOTFS`), if one was found.
    pub bootfs: Option<DatasetName>,
    /// The host ID that host ID matching wrote, if it matched one.
    pub spl_hostid: Option<HostId>,
}

/// The start-up stage `init.d/50-import-pools`.
pub fn stage(
    system: &System<'_>,
    state: &State<'_>,
    settings: &ImportSettings,
    operator: &mut dyn Operator,
) -> Imported {
    let mut imported = Imported::default();
    let mut retry = ImportLoop::new(settings);
    loop {
        // On a retry, first unlock LUKS volumes on disks that appeared during
        // the delay, so that a pool is not imported without them.
        if retry.failures() > 0 && operator.unlock_late() == Late::Unlocked {
            zbm_trace::info!("unlocked LUKS volumes that appeared late");
        }
        match attempt(system, state, settings, &mut retry, &mut imported) {
            Next::Done => break,
            Next::Again => {}
            Next::Failed { attempt } => failed(system, settings, &retry, operator, attempt),
        }
    }

    restrict_unhealthy(system.runner, state);
    // `zdebug && zdebug "$( zreport )"`: the first call writes an empty line.
    // The system report itself arrives with the recovery tools (package F).
    if zbm_trace::enabled(Level::Debug) {
        zbm_trace::debug!("");
    }
    check_unsupported(system.runner, state, settings, operator);
    imported.bootfs = find_bootfs(system.runner, state, settings);
    imported
}

/// One import attempt, and host ID matching when the loop asks for it.
fn attempt(
    system: &System<'_>,
    state: &State<'_>,
    settings: &ImportSettings,
    retry: &mut ImportLoop,
    imported: &mut Imported,
) -> Next {
    if let Some(target) = retry.target() {
        zbm_trace::debug!("attempting to import preferred pool {target}");
    }
    // Whether anything was imported is what counts, not the status.
    let _ = import_pool(
        system,
        state,
        Import::target(retry.target()).policy(settings.policy),
    );
    let found = if imported_pools(system.runner).is_empty() {
        Found::Nothing
    } else {
        Found::Pools
    };
    if found == Found::Pools && settings.requirement == Requirement::Only {
        let target = retry.target().map_or("", PoolRef::as_str);
        zbm_trace::debug!("only importing {target}");
    }
    match retry.imported(found) {
        AfterImport::Next(next) => next,
        AfterImport::MatchHostid => {
            let target = retry.target().cloned();
            let matched = match match_hostid(system, state, settings, target.as_ref()) {
                Some(found) => {
                    zbm_trace::debug!("match_hostid returned: {};{}", found.pool, found.hostid);
                    if let Err(error) = state.set_spl_hostid(found.hostid.as_str()) {
                        zbm_trace::error!("{}", error.to_string());
                    }
                    imported.spl_hostid = Some(found.hostid);
                    Matched::Pool
                }
                None => Matched::Nothing,
            };
            retry.matched(matched)
        }
    }
}

/// A failed attempt: late LUKS volumes, or the prompt and, on ESCAPE, the
/// recovery shell.
fn failed(
    system: &System<'_>,
    settings: &ImportSettings,
    retry: &ImportLoop,
    operator: &mut dyn Operator,
    attempt: u64,
) {
    zbm_trace::info!("unable to import a pool on attempt {attempt}");
    if operator.unlock_late() == Late::Unlocked {
        zbm_trace::info!("unlocked LUKS volumes that appeared late, retrying import");
        return;
    }
    if operator.retry(retry.target(), settings.retry_delay) == Answer::Continue {
        return;
    }
    log_unimportable(system.runner);
    operator.emergency_shell("unable to successfully import a pool");
}

/// Restricts read-write access to every pool that is not `ONLINE`.
fn restrict_unhealthy(runner: &dyn Runner, state: &State<'_>) {
    let list =
        ZpoolCommand::new(runner).text(&["list", "-H", "-o", "name,health"], Stderr::Console);
    for line in tab_lines(&stdout(list)) {
        let (pool, health) = tab_fields(&line);
        if health != "ONLINE" {
            if let Err(error) = state.restrict(pool) {
                zbm_trace::error!("{}", error.to_string());
            }
            zbm_trace::error!("prohibiting read/write operations on {pool}");
        }
    }
}

/// Restricts pools with features this build of ZFS does not support, and
/// tells the user.
fn check_unsupported(
    runner: &dyn Runner,
    state: &State<'_>,
    settings: &ImportSettings,
    operator: &mut dyn Operator,
) {
    let get = ZpoolCommand::new(runner).text(
        &["get", "all", "-H", "-o", "name,property"],
        Stderr::Console,
    );
    let mut found = Found::Nothing;
    for line in tab_lines(&stdout(get)) {
        let (pool, property) = tab_fields(&line);
        if !property.contains("unsupported@") {
            continue;
        }
        zbm_trace::error!("unsupported property: {property}");
        if !state.is_degraded(pool)
            && let Err(error) = state.restrict(pool)
        {
            zbm_trace::error!("{}", error.to_string());
        }
        found = Found::Pools;
    }
    if found == Found::Nothing {
        return;
    }
    let advice = match settings.build {
        Build::Release => "Check for a new binary release of ZFSBootMenu",
        Build::Local => "Upgrade ZFS modules in ZFSBootMenu with generate-zbm",
    };
    zbm_trace::error!("Unsupported features detected, {advice}");
    operator.unsupported_features(advice);
}

/// The default boot environment, recorded in the state directory.
fn find_bootfs(
    runner: &dyn Runner,
    state: &State<'_>,
    settings: &ImportSettings,
) -> Option<DatasetName> {
    let bootfs = choose_bootfs(runner, settings)?;
    if let Err(error) = state.set_bootfs(bootfs.as_str()) {
        zbm_trace::error!("{}", error.to_string());
    }
    Some(bootfs)
}

/// The default boot environment: `zbm.prefer` naming a filesystem, or else the
/// `bootfs` property of the preferred pool or of the first imported pool that
/// sets it. It only reads.
pub fn choose_bootfs(runner: &dyn Runner, settings: &ImportSettings) -> Option<DatasetName> {
    let preferred = match &settings.prefer_bootfs {
        BootfsPreference::FromPool => return pool_bootfs(runner, settings.prefer_pool.as_ref()),
        BootfsPreference::Dataset(dataset) => dataset.as_str(),
        BootfsPreference::Invalid(text) => text.as_str(),
    };
    if ZfsCommand::new(runner).is_filesystem(preferred) {
        return preferred.parse().ok();
    }
    zbm_trace::error!("bootfs value '{preferred}' not a ZFS dataset");
    None
}

/// The `bootfs` property of `pool`, or of the first imported pool that sets
/// it.
fn pool_bootfs(runner: &dyn Runner, pool: Option<&PoolRef>) -> Option<DatasetName> {
    let pool = pool.map_or("--", PoolRef::as_str);
    let list =
        ZpoolCommand::new(runner).text(&["list", "-H", "-o", BOOTFS.name, pool], Stderr::Console);
    // The first line that is not `-` decides; an empty one means none.
    let bootfs = here_lines(&stdout(list))
        .into_iter()
        .find(|line| line != "-")
        .filter(|line| !line.is_empty())?;
    match bootfs.parse() {
        Ok(dataset) => Some(dataset),
        Err(error) => {
            zbm_trace::error!("{error}");
            None
        }
    }
}

#[cfg(test)]
mod tests;
