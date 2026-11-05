//! Importing a pool from the menu ([`Action::Import`]), on the machine.
//!
//! - A pool that is imported is exported and imported again with the
//!   options, under `zbm.import_policy`, as the legacy menu imported it
//!   read/write or read-only again, or rewound it to its checkpoint
//!   ([`reimport`]): a read/write import is refused by the pool's
//!   `org.zfsbootmenu:readonly` property or a degraded health, and asks
//!   about a hibernated system, and a rewind is refused when the pool has
//!   no checkpoint, before anything is exported; a failed import imports
//!   the pool read-only again.
//! - A pool that is not imported is imported read-only first, under the
//!   options' policy. Under [`ImportPolicy::Hostid`], one this machine's
//!   host ID does not own is imported with the host ID of the host that
//!   last imported it, which is recorded for the boot, as start-up does.
//!   A read/write import or a rewind then follows as for an imported
//!   pool, with its checks, under the options' policy.
//!
//! The menu sends every import on the console ([`Action::needs_console`]),
//! so it is never cancelled between the export and the import.
//!
//! [`Action::Import`]: crate::model::Action::Import
//! [`Action::needs_console`]: crate::model::Action::needs_console

use zbm_platform::{Context, Runner};
use zbm_pool::ImportSettings;
use zbm_pool::import::{import_pool, imported_pools, match_hostid};
use zbm_pool::writable::{is_writable, reimport};
use zbm_tools::console::ConsoleInteraction;
use zbm_types::{ImportPolicy, PoolName};
use zbm_zfs::{Stderr, ZfsChange, ZfsText};

use super::needs::OnMachine;
use super::{Changes, Failure, done};
use crate::model::{Access, Event, ImportOptions, Step, StepKind};
use crate::volumes::causes;

impl Changes<'_, '_> {
    /// Imports `pool` with `options`.
    pub(super) fn import(
        &mut self,
        pool: &PoolName,
        options: ImportOptions,
    ) -> Result<Vec<Event>, Failure> {
        let access = match options.access {
            Access::ReadOnly => "read-only",
            Access::ReadWrite => "read/write",
        };
        let (what, imported) = if options.rewind {
            (
                format!("rewind {pool} to its checkpoint"),
                format!("{pool} is rewound to its checkpoint and imported {access}"),
            )
        } else {
            (
                format!("import {pool} {access}"),
                format!("{pool} is imported {access}"),
            )
        };
        self.needs
            .import(pool, options)
            .map_err(|why| Failure::new(format!("unable to {what}"), why))?;
        Ok(done(imported))
    }
}

/// Whether `pool` is imported.
fn is_imported(runner: &dyn Runner, pool: &PoolName) -> bool {
    imported_pools(runner).contains(pool)
}

/// Whether `pool`, imported, has a checkpoint to rewind to: its
/// `checkpoint` property has a size.
fn has_checkpoint(runner: &dyn Runner, pool: &PoolName) -> bool {
    ZfsText::new(runner)
        .pool_value(pool, "checkpoint", Stderr::Hidden)
        .is_some_and(|size| !size.is_empty() && size != "-")
}

/// Why a rewind of `pool`, which has no checkpoint, is refused.
fn no_checkpoint(pool: &PoolName) -> Vec<String> {
    vec![format!("{pool} has no checkpoint")]
}

/// Whether the import asks only for what a read-only import gives.
fn is_read_only(options: ImportOptions) -> bool {
    options.access == Access::ReadOnly && !options.rewind
}

/// The read-only import, without a rewind, that a pool which is not
/// imported is imported with first, under the options' policy.
fn read_only(options: ImportOptions) -> ImportOptions {
    ImportOptions {
        access: Access::ReadOnly,
        rewind: false,
        ..options
    }
}

/// Imports `pool` with `options` on the machine, on its console; without
/// it, nothing is done.
pub(super) fn import(
    machine: &OnMachine<'_, '_>,
    pool: &PoolName,
    options: ImportOptions,
) -> Result<(), Vec<String>> {
    if !machine.asks {
        return Err(vec![format!("importing {pool} takes the console")]);
    }
    let policy = if is_imported(machine.system.runner, pool) {
        machine.settings.import_policy
    } else {
        first(machine, pool, options.policy)?;
        if is_read_only(options) {
            return Ok(());
        }
        options.policy
    };
    again(machine, pool, options, policy)
}

/// Imports `pool`, which is not imported, read-only under `policy`. Under
/// [`ImportPolicy::Hostid`], when that fails, it is imported with the host
/// ID of the host that last imported it, which is recorded for the boot,
/// as start-up does.
fn first(
    machine: &OnMachine<'_, '_>,
    pool: &PoolName,
    policy: ImportPolicy,
) -> Result<(), Vec<String>> {
    let context = Context::new(*machine.system);
    let import = ImportOptions::default().import(pool, policy);
    let Err(error) = import_pool(&context, machine.state, import) else {
        return Ok(());
    };
    if policy != ImportPolicy::Hostid {
        return Err(causes(&error));
    }
    let settings = ImportSettings {
        policy: ImportPolicy::Hostid,
        ..machine.import.clone()
    };
    match match_hostid(&context, machine.state, &settings, Some(pool)) {
        Some(found) => {
            if let Err(error) = machine.state.set_spl_hostid(found.hostid.as_str()) {
                tracing::warn!(%error, %pool, "recording the host ID of the pool's host");
            }
            Ok(())
        }
        // Its probe imports the pool read-only when nothing refuses it.
        None if is_imported(context.runner(), pool) => Ok(()),
        None => {
            let mut why = causes(&error);
            why.push(format!(
                "the host ID that last imported {pool} did not import it either"
            ));
            Err(why)
        }
    }
}

/// Exports `pool`, which is imported, and imports it again with
/// `options`, under `policy`. A rewind of a pool with no checkpoint is
/// refused before anything is exported.
fn again(
    machine: &OnMachine<'_, '_>,
    pool: &PoolName,
    options: ImportOptions,
    policy: ImportPolicy,
) -> Result<(), Vec<String>> {
    if options.rewind && !has_checkpoint(machine.system.runner, pool) {
        return Err(no_checkpoint(pool));
    }
    let context = Context::new(*machine.system);
    let interaction = ConsoleInteraction::new(*machine.system);
    reimport(
        &context,
        &interaction,
        machine.state,
        pool,
        policy,
        options.access.into(),
        options.rewinds(),
    )
    .map_err(|error| causes(&error))
}

/// The steps of importing `pool` with `options` on the machine, which is
/// only read. An imported pool is exported and imported again, unless it
/// has the access asked already and nothing is rewound. One that is not
/// imported is imported read-only first, then, under
/// [`ImportPolicy::Hostid`], with the host ID of its last host when that
/// fails, and then exported and imported again for read/write or a
/// rewind.
///
/// # Errors
///
/// The rewind of an imported pool with no checkpoint.
pub(super) fn steps(
    machine: &OnMachine<'_, '_>,
    pool: &PoolName,
    options: ImportOptions,
) -> Result<Vec<Step>, Vec<String>> {
    let runner = machine.system.runner;
    let mut steps = Vec::new();
    let policy = if is_imported(runner, pool) {
        if options.rewind && !has_checkpoint(runner, pool) {
            return Err(no_checkpoint(pool));
        }
        let writes = options.access == Access::ReadWrite;
        if !options.rewind && is_writable(runner, pool) == writes {
            return Ok(steps);
        }
        machine.settings.import_policy
    } else {
        steps.push(import_step(pool, read_only(options), options.policy));
        if options.policy == ImportPolicy::Hostid {
            steps.push(Step {
                kind: StepKind::AdoptHostid(pool.clone()),
                command: None,
            });
        }
        if is_read_only(options) {
            return Ok(steps);
        }
        options.policy
    };
    let export = ZfsChange::Export(pool.clone());
    steps.push(Step {
        command: Some(export.to_string()),
        kind: StepKind::Export(pool.clone()),
    });
    steps.push(import_step(pool, options, policy));
    Ok(steps)
}

/// The step that imports `pool` with `options` under `policy`.
fn import_step(pool: &PoolName, options: ImportOptions, policy: ImportPolicy) -> Step {
    let arguments = options.import(pool, policy).arguments().join(" ");
    Step {
        kind: StepKind::Import {
            pool: pool.clone(),
            options,
        },
        command: Some(format!("zpool {arguments}")),
    }
}

#[cfg(test)]
mod tests;
