//! What stops a change, and its steps, in the words of its review page.

use zbm_types::{ImportPolicy, Printable};

use super::{Part, part};
use crate::menu::format::capitalised;
use crate::menu::model::host_text;
use crate::menu::theme::Role;
use crate::menu::world::{Access, ImportOptions, Step, StepKind, Stop};
use crate::text::text;

/// What stops a change, in words.
pub(super) fn stop(stop: &Stop) -> String {
    match stop {
        Stop::Clone { snapshot, clone } => text!(
            "review-refused-clone",
            snapshot = snapshot.name(),
            clone = clone.printable().to_string()
        ),
        Stop::Hold {
            snapshot,
            tag: Some(tag),
        } => text!(
            "review-refused-hold",
            snapshot = snapshot.name(),
            tag = tag.printable().to_string()
        ),
        Stop::Hold {
            snapshot,
            tag: None,
        } => {
            text!("review-refused-held", snapshot = snapshot.name())
        }
        Stop::Active { pool, host } => {
            let host = host
                .as_ref()
                .map_or_else(|| text!("pool-host-unknown"), host_text);
            text!(
                "review-refused-active",
                pool = pool.to_string(),
                host = host
            )
        }
    }
}

/// What a step does, in words.
fn step(kind: &StepKind) -> String {
    match kind {
        StepKind::ImportReadWrite(pool) => text!("review-step-read-write", pool = pool.to_string()),
        StepKind::LoadKey(dataset) => text!("review-step-load-key", dataset = dataset.to_string()),
        StepKind::SetDefault { pool, environment } => text!(
            "review-step-default",
            pool = pool.to_string(),
            environment = environment.as_str()
        ),
        StepKind::SetPoolProperty { pool, property } => text!(
            "review-step-set",
            target = pool.to_string(),
            property = property.as_str()
        ),
        StepKind::SetProperty { dataset, property } => text!(
            "review-step-set",
            target = dataset.to_string(),
            property = property.as_str()
        ),
        StepKind::InheritProperty { dataset, property } => text!(
            "review-step-inherit",
            dataset = dataset.to_string(),
            property = property.as_str()
        ),
        StepKind::Snapshot(snapshot) => {
            text!("review-step-snapshot", snapshot = snapshot.to_string())
        }
        StepKind::Rollback(snapshot) => text!(
            "review-step-rollback",
            dataset = snapshot.dataset().to_string(),
            snapshot = snapshot.name()
        ),
        StepKind::Destroy(snapshots) => text!("review-step-destroy", count = snapshots.len()),
        StepKind::Clone { snapshot, target } | StepKind::Copy { snapshot, target } => text!(
            "review-step-clone",
            snapshot = snapshot.to_string(),
            target = target.to_string()
        ),
        StepKind::Promote(dataset) => text!("review-step-promote", dataset = dataset.to_string()),
        StepKind::InheritKey(dataset) => {
            text!("review-step-inherit-key", dataset = dataset.to_string())
        }
        StepKind::Export(pool) => text!("review-step-export", pool = pool.to_string()),
        StepKind::Import { pool, options } => import_step(pool, options),
        StepKind::AdoptHostid(pool) => text!("review-step-adopt-hostid", pool = pool.to_string()),
        StepKind::Boot(environment) => {
            text!("review-step-boot", environment = environment.to_string())
        }
        StepKind::Restore { snapshot, paths } => text!(
            "review-step-restore",
            snapshot = snapshot.to_string(),
            count = paths.len()
        ),
    }
}

/// `access`, in words: read-only, or read/write.
pub(super) fn access(access: Access) -> String {
    match access {
        Access::ReadOnly => text!("pool-read-only"),
        Access::ReadWrite => text!("pool-read-write"),
    }
}

/// What the import of `pool` with `options` does, in words.
fn import_step(pool: &zbm_types::PoolName, options: &ImportOptions) -> String {
    let pool = pool.to_string();
    let access = access(options.access);
    if options.rewind {
        text!("review-step-rewind", pool = pool)
    } else if options.policy == ImportPolicy::Force {
        text!("review-step-force-import", pool = pool, access = access)
    } else {
        text!("review-step-import", pool = pool, access = access)
    }
}

/// The steps, each what it does, then its command, muted.
pub(super) fn steps(steps: &[Step]) -> Vec<Part> {
    let mut parts = Vec::new();
    for (number, each) in steps.iter().enumerate() {
        let what = format!("{}. {}", number + 1, capitalised(&step(&each.kind)));
        parts.push(part(what, Role::Text));
        if let Some(command) = &each.command {
            parts.push(part(format!("   {}", Printable(command)), Role::Muted));
        }
    }
    parts
}
