//! What undoes a change that loses nothing, read from the machine before
//! it is made (ADR 0016, answer 5):
//!
//! - a snapshot is undone by destroying it;
//! - the import of a pool that is not imported by its export; as the host
//!   that last imported it, of another system's pool, by nothing, as the
//!   host ID it adopts stays adopted ([`Review::host_id_stays`]);
//! - the import of a pool that is, with the other access, by importing it
//!   again with the access it has;
//! - a new default environment, by the old one, when the pool had one;
//! - a default kernel, by inheriting it again, when it was inherited;
//! - a command line saved or inherited, by the line set on the
//!   environment before, or by inheriting it again, when it was
//!   inherited.
//!
//! A clone, a copy, a promote, an export, a restore and the boot of a
//! snapshot have no undo: no action destroys an environment, and a scan
//! finds an exported pool. A read that fails leaves the undo unknown,
//! `None`, and is logged.

use zbm_config::spec::properties::{COMMANDLINE, KERNEL};
use zbm_types::{DatasetName, ImportPolicy, PoolName, Untrusted};
use zbm_zfs::{Source, Zfs};

use crate::model::{Access, Action, ImportOptions, Review};

/// The review of `action`, a change that loses nothing: what undoes it.
pub(super) fn review(zfs: &dyn Zfs, action: &Action) -> Review {
    match action {
        Action::CreateSnapshot(snapshot) => undone_by(Action::Destroy(vec![snapshot.clone()])),
        Action::Import(pool, options) => import(zfs, pool, *options),
        Action::SetDefaultEnvironment(environment) => default(zfs, environment),
        Action::SetDefaultKernel(environment, _) => kernel(zfs, environment),
        Action::SetCommandLine(environment, _) | Action::InheritCommandLine(environment) => {
            command_line(zfs, environment)
        }
        _ => Review::default(),
    }
}

/// The review of a change that `undo` undoes.
fn undone_by(undo: Action) -> Review {
    Review {
        undo: Some(Box::new(undo)),
        ..Review::default()
    }
}

/// The review of importing `pool` with `options`, neither forced nor
/// rewound: an imported pool given the other access is given back the one
/// it has; one given the access it has is not changed. Another pool of its
/// name imported here stops the import, which has no undo then.
fn import(zfs: &dyn Zfs, pool: &PoolName, options: ImportOptions) -> Review {
    let pools = match zfs.pools() {
        Ok(pools) => pools,
        Err(error) => {
            tracing::warn!(%error, %pool, "the imported pools could not be listed");
            return Review::default();
        }
    };
    let Some(imported) = pools.iter().find(|imported| imported.name == *pool) else {
        return exported(zfs, pool, options);
    };
    let access = if imported.read_only {
        Access::ReadOnly
    } else {
        Access::ReadWrite
    };
    if access == options.access || options.id.is_some_and(|id| imported.guid != id) {
        return Review::default();
    }
    undone_by(Action::Import(
        pool.clone(),
        ImportOptions { access, ..options },
    ))
}

/// The review of importing `pool`, which is not imported, with `options`:
/// undone by its export, unless, as the host that last imported it, it is
/// another system's, whose host ID it adopts, which stays adopted. Whose
/// it is, `zpool import` lists; when that fails, or does not list it, the
/// undo is unknown.
fn exported(zfs: &dyn Zfs, pool: &PoolName, options: ImportOptions) -> Review {
    if options.policy != ImportPolicy::Hostid {
        return undone_by(Action::Export(pool.clone()));
    }
    let listed = match zfs.importable() {
        Ok(listed) => listed,
        Err(error) => {
            tracing::warn!(%error, %pool, "the pools could not be scanned");
            return Review::default();
        }
    };
    let found = listed.iter().find(|listed| {
        options
            .id
            .map_or(listed.name == *pool, |id| listed.id == id)
    });
    match found {
        Some(found) if found.foreign => Review {
            host_id_stays: true,
            ..Review::default()
        },
        Some(_) => undone_by(Action::Export(pool.clone())),
        None => Review::default(),
    }
}

/// The review of making `environment` the default of its pool: undone by
/// the default it replaces, if the pool has another.
fn default(zfs: &dyn Zfs, environment: &DatasetName) -> Review {
    let pool = environment.pool();
    let old = zfs
        .pool_property(&pool, "bootfs")
        .inspect_err(|error| tracing::warn!(%error, %pool, "the default is unknown"))
        .ok()
        .and_then(|bootfs| bootfs.value().and_then(|value| value.parse().ok()))
        .filter(|old: &DatasetName| old != environment);
    old.map_or_else(Review::default, |old| {
        undone_by(Action::SetDefaultEnvironment(old))
    })
}

/// The review of setting the default kernel of `environment`: undone by
/// inheriting it again, when it is not set on the environment itself.
fn kernel(zfs: &dyn Zfs, environment: &DatasetName) -> Review {
    let inherits = zfs
        .property(environment, KERNEL.name)
        .inspect_err(|error| {
            tracing::warn!(%error, %environment, "the default kernel's origin is unknown");
        })
        .is_ok_and(|kernel| {
            matches!(
                kernel.source,
                Source::Inherited(_) | Source::Default | Source::None
            )
        });
    if inherits {
        undone_by(Action::InheritDefaultKernel(environment.clone()))
    } else {
        Review::default()
    }
}

/// The review of saving or inheriting the command line of `environment`:
/// undone by the line set on it before, as it was set, or by inheriting
/// it again, when it inherits it or has none.
fn command_line(zfs: &dyn Zfs, environment: &DatasetName) -> Review {
    let line = match zfs.property(environment, COMMANDLINE.name) {
        Ok(line) => line,
        Err(error) => {
            tracing::warn!(%error, %environment, "the command line is unknown");
            return Review::default();
        }
    };
    let set = matches!(
        line.source,
        Source::Local | Source::Received | Source::Temporary
    );
    match line.value().filter(|_| set) {
        Some(old) => undone_by(Action::SetCommandLine(
            environment.clone(),
            Untrusted::new(old.to_owned()),
        )),
        None => undone_by(Action::InheritCommandLine(environment.clone())),
    }
}

#[cfg(test)]
mod tests;
