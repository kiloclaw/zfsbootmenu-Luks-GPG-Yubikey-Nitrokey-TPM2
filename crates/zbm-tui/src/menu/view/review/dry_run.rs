//! What the dry run of a change says that its review page does not (ADR
//! 0007, point 5; ADR 0016, point 12): a title that names the change and
//! its target, and its undo, or that it loses nothing; of a command line
//! saved, the line it replaces and what still comes before it. A page
//! that loses nothing and asks for no name is not drawn as a danger. The
//! undo line of every page is said here: a review page names the undos a
//! dry run does, and says that it has none as it did.

use zbm_types::ImportPolicy;

use super::steps::access;
use super::{Part, part};
use crate::menu::model::{Model, Reviewing, own_command_line};
use crate::menu::theme::Role;
use crate::menu::world::{Action, Origin, Review};
use crate::text::text;

/// The title of the dry run of `action`, which names the change and its
/// target; `None` for an action without a plan, and for those whose
/// review page has its own title. Every kind of action is said here, so
/// that a new one is given a title or none.
pub(super) fn title(action: &Action) -> Option<String> {
    match action {
        Action::CreateSnapshot(_)
        | Action::Clone(..)
        | Action::CloneAndPromote(..)
        | Action::Duplicate(..)
        | Action::BootSnapshot(..)
        | Action::RestoreFiles { .. } => made(action),
        Action::SetDefaultEnvironment(_)
        | Action::SetDefaultKernel(..)
        | Action::InheritDefaultKernel(_)
        | Action::SetCommandLine(..)
        | Action::InheritCommandLine(_)
        | Action::Import(..)
        | Action::Export(_) => set(action),
        // Their review pages have titles of their own; the others have no
        // plan.
        Action::Rollback(_)
        | Action::Destroy(_)
        | Action::Boot { .. }
        | Action::Diff(..)
        | Action::ResetBootTries(_)
        | Action::Reseal(_)
        | Action::Chroot(_)
        | Action::Unlock { .. }
        | Action::SkipUnlock(_)
        | Action::Answer(_)
        | Action::UnlockAll
        | Action::Open { .. }
        | Action::OpenWithKeyFile { .. }
        | Action::Close(_)
        | Action::Shell
        | Action::FirmwareSetup
        | Action::Reboot
        | Action::PowerOff
        | Action::Plan(_)
        | Action::ScanPools
        | Action::LoadDeviceLog(_)
        | Action::LoadSnapshots(_)
        | Action::LoadHistory => None,
    }
}

/// The title of the dry run of `action`, a change that makes something
/// of a snapshot.
fn made(action: &Action) -> Option<String> {
    Some(match action {
        Action::CreateSnapshot(snapshot) => {
            text!("dry-run-snapshot", snapshot = snapshot.to_string())
        }
        Action::Clone(snapshot, target) => text!(
            "dry-run-clone",
            snapshot = snapshot.to_string(),
            environment = target.to_string()
        ),
        Action::CloneAndPromote(snapshot, target) => text!(
            "dry-run-clone-promote",
            snapshot = snapshot.to_string(),
            environment = target.to_string()
        ),
        Action::Duplicate(snapshot, target) => text!(
            "dry-run-duplicate",
            snapshot = snapshot.to_string(),
            environment = target.to_string()
        ),
        Action::BootSnapshot(snapshot, target) => text!(
            "dry-run-boot-clone",
            snapshot = snapshot.to_string(),
            environment = target.to_string()
        ),
        Action::RestoreFiles { snapshot, paths } => text!(
            "dry-run-restore",
            snapshot = snapshot.to_string(),
            count = paths.len()
        ),
        _ => return None,
    })
}

/// The title of the dry run of `action`, a change of a default or of a
/// pool.
fn set(action: &Action) -> Option<String> {
    Some(match action {
        Action::SetDefaultEnvironment(environment) => {
            text!("dry-run-default", environment = environment.to_string())
        }
        Action::SetDefaultKernel(environment, kernel) => text!(
            "dry-run-kernel",
            environment = environment.to_string(),
            kernel = kernel.printable().to_string()
        ),
        Action::InheritDefaultKernel(environment) => {
            text!(
                "dry-run-inherit-kernel",
                environment = environment.to_string()
            )
        }
        Action::SetCommandLine(environment, _) => {
            text!(
                "dry-run-command-line",
                environment = environment.to_string()
            )
        }
        Action::InheritCommandLine(environment) => text!(
            "dry-run-inherit-command-line",
            environment = environment.to_string()
        ),
        Action::Import(pool, options) if options.policy == ImportPolicy::Hostid => text!(
            "dry-run-import-last-host",
            pool = pool.to_string(),
            access = access(options.access)
        ),
        Action::Import(pool, options) => text!(
            "dry-run-import",
            pool = pool.to_string(),
            access = access(options.access)
        ),
        Action::Export(pool) => text!("dry-run-export", pool = pool.to_string()),
        _ => return None,
    })
}

/// Whether the page of `reviewing` is drawn as a danger, its frame and
/// its verb: a review page, which asks for a name, and a dry run that
/// loses something.
pub(super) fn is_danger(reviewing: &Reviewing) -> bool {
    reviewing.typed.is_some()
        || reviewing
            .plan
            .as_ref()
            .is_some_and(|plan| !plan.review.loses.is_empty())
}

/// What undoes the change of `review`, the plan of `reviewing`: the
/// action, named; that the host ID it adopts stays adopted; on a dry run,
/// that it loses nothing; or, as it loses what cannot be brought back, or
/// on a review page, that nothing does.
pub(super) fn undo(reviewing: &Reviewing, review: &Review) -> Part {
    let named = match review.undo.as_deref() {
        Some(Action::Export(pool)) => text!("review-undo-export", pool = pool.to_string()),
        Some(Action::Destroy(snapshots)) => text!(
            "review-undo-destroy",
            snapshot = snapshots
                .first()
                .map(ToString::to_string)
                .unwrap_or_default()
        ),
        Some(Action::Import(pool, options)) => text!(
            "review-undo-import",
            pool = pool.to_string(),
            access = access(options.access)
        ),
        Some(Action::SetDefaultEnvironment(environment)) => {
            text!("review-undo-default", environment = environment.to_string())
        }
        Some(Action::InheritDefaultKernel(environment)) => text!(
            "review-undo-inherit-kernel",
            environment = environment.to_string()
        ),
        Some(Action::SetCommandLine(environment, line)) => text!(
            "review-undo-command-line",
            environment = environment.to_string(),
            line = line.printable().to_string()
        ),
        Some(Action::InheritCommandLine(environment)) => text!(
            "review-undo-inherit-command-line",
            environment = environment.to_string()
        ),
        Some(_) => text!("review-undo"),
        None if review.host_id_stays => {
            return part(text!("review-host-id-stays"), Role::Warning);
        }
        None if review.loses.is_empty() && reviewing.typed.is_none() => {
            text!("dry-run-loses-nothing")
        }
        None => return part(text!("review-no-undo"), Role::Warning),
    };
    part(named, Role::Text)
}

/// What the dry run of a command line saved or inherited says after its
/// steps, before its undo: the line its environment boots with until then,
/// and where it comes from; then that a kernel's own `.kcl` file and
/// `zbm.kcl_override` still come before the saved line, and that with
/// measured boot PCR 8 changes for every later boot. Those two are said
/// always: the menu knows neither a kernel's files nor whether the image
/// measures.
pub(super) fn command_line(model: &Model, reviewing: &Reviewing) -> Vec<Part> {
    let (Action::SetCommandLine(name, _) | Action::InheritCommandLine(name)) = &reviewing.action
    else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    let environments = &model.snapshot.environments;
    if let Some(environment) = environments.iter().find(|known| known.name == *name) {
        let line = own_command_line(environment);
        let line = if line.get().is_empty() {
            text!("value-empty")
        } else {
            line.printable().to_string()
        };
        let origin = match &environment.command_line.origin {
            Origin::Local => text!("origin-local"),
            Origin::Inherited(from) => text!("origin-inherited", from = from.to_string()),
            Origin::Default => text!("label-default"),
        };
        let was = text!("dry-run-command-line-was", line = line, origin = origin);
        parts.push(part(was, Role::Text));
    }
    parts.push(part(text!("dry-run-command-line-kernels"), Role::Muted));
    parts.push(part(text!("dry-run-command-line-measured"), Role::Muted));
    parts
}

#[cfg(test)]
mod tests;
