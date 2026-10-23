//! The intents of the main, kernels, pools and recovery views.

use zbm_types::ImportPolicy;

use super::dialogs;
use crate::menu::keymap::{Context, Intent};
use crate::menu::model::{Choice, Editor, Model, RECOVERY, RecoveryTool, Screen, Then};
use crate::menu::world::{Action, Environment, Method, Outcome};

/// What `intent` does on the current screen.
pub(super) fn intent(model: &mut Model, intent: Intent) -> Vec<Action> {
    match model.screen {
        Screen::Main => main(model, intent),
        Screen::Kernels => kernels(model, intent),
        Screen::Snapshots => super::snapshots::intent(model, intent),
        Screen::Pools => pools(model, intent),
        Screen::Recovery => recovery(model, intent),
        _ => Vec::new(),
    }
}

/// The screen a key map context belongs to.
pub(super) fn screen_of(context: Context) -> Screen {
    match context {
        Context::Snapshots => Screen::Snapshots,
        Context::Kernels => Screen::Kernels,
        Context::Pools => Screen::Pools,
        Context::Recovery => Screen::Recovery,
        Context::Log => Screen::Log,
        Context::Startup => Screen::Startup,
        _ => Screen::Main,
    }
}

/// The boot of `environment` with `kernel`, and its command line for this
/// boot.
pub(super) fn boot(model: &Model, environment: &Environment, kernel: Option<String>) -> Action {
    let line = model.command_line(environment);
    let edited = line != environment.command_line.value;
    Action::Boot {
        environment: environment.name.clone(),
        kernel,
        command_line: edited.then_some(line),
    }
}

/// Opens `screen` on `environment`: its snapshots, kernels or command line.
pub(super) fn focus(model: &mut Model, environment: zbm_types::DatasetName, screen: Screen) {
    model.focus = Some(environment);
    model.snapshots = Default::default();
    model.kernels = Default::default();
    model.marked.clear();
    model.refresh();
    if screen == Screen::Kernels {
        let default = model
            .focused()
            .and_then(|environment| environment.kernels.iter().position(|k| k.is_default));
        model.kernels.selected = default.unwrap_or(0);
    }
    if screen == Screen::CommandLine
        && let Some(environment) = model.focused().cloned()
    {
        model.editor = Some(Editor {
            text: model.command_line(&environment),
            environment: environment.name,
            kernel: None,
        });
    }
    model.screen = screen;
}

/// The main view.
fn main(model: &mut Model, intent: Intent) -> Vec<Action> {
    let Some(environment) = model.selected_environment().cloned() else {
        return Vec::new();
    };
    let name = environment.name.clone();
    match intent {
        Intent::Primary | Intent::Boot => return vec![boot(model, &environment, None)],
        Intent::Kernels => focus(model, name, Screen::Kernels),
        Intent::Snapshots => focus(model, name, Screen::Snapshots),
        Intent::Edit => focus(model, name, Screen::CommandLine),
        Intent::SetDefault => return vec![Action::SetDefaultEnvironment(name)],
        Intent::Pools => model.screen = Screen::Pools,
        Intent::ReadWrite => return vec![Action::ImportReadWrite(name.pool())],
        Intent::Chroot => return vec![Action::Chroot(name.to_string())],
        Intent::NewSnapshot => return vec![Action::CreateSnapshot(name)],
        Intent::Shell => return vec![Action::Shell],
        Intent::Sort => {}
        Intent::RevertCommandLine => {
            model.edited.retain(|(edited, _)| *edited != name);
            model.notification = Some(Outcome::Done(format!("command line of {name} reverted")));
        }
        _ => {}
    }
    Vec::new()
}

/// The kernels of the focused environment.
fn kernels(model: &mut Model, intent: Intent) -> Vec<Action> {
    let Some(environment) = model.focused().cloned() else {
        return Vec::new();
    };
    let Some(kernel) = model
        .kernels
        .selected_item()
        .and_then(|index| environment.kernels.get(index))
        .cloned()
    else {
        return Vec::new();
    };
    let name = environment.name.clone();
    match intent {
        Intent::Primary | Intent::Boot => vec![boot(model, &environment, Some(kernel.version))],
        Intent::SetDefault if kernel.is_default => vec![Action::InheritDefaultKernel(name)],
        Intent::SetDefault => vec![Action::SetDefaultKernel(name, kernel.version)],
        Intent::InheritDefault => vec![Action::InheritDefaultKernel(name)],
        Intent::Edit => {
            focus(model, name, Screen::CommandLine);
            if let Some(editor) = &mut model.editor {
                editor.kernel = Some(kernel.version);
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Pools and volumes.
fn pools(model: &mut Model, intent: Intent) -> Vec<Action> {
    if let Some(volume) = model.selected_volume().map(str::to_owned) {
        if matches!(intent, Intent::Primary | Intent::Unlock) {
            unlock(model, &volume);
        }
        return Vec::new();
    }
    let Some(pool) = model.selected_pool().cloned() else {
        return Vec::new();
    };
    match intent {
        Intent::Primary => dialogs::pool_details(model, &pool),
        Intent::Import => import_choices(model, &pool),
        Intent::ReadWrite => return vec![Action::ImportReadWrite(pool)],
        Intent::Rewind => {
            model.dialog = Some(dialogs::confirm(Then::Confirm {
                action: Action::RewindCheckpoint(pool.clone()),
                title: format!("Rewind {pool} to its checkpoint"),
                lines: vec![
                    format!("Every change to {pool} since its checkpoint is lost."),
                    "The checkpoint itself is discarded.".to_owned(),
                ],
                name: pool.to_string(),
            }));
        }
        _ => {}
    }
    Vec::new()
}

/// Opens the unlock dialog of `volume` with its first method.
pub(super) fn unlock(model: &mut Model, volume: &str) {
    let method = model
        .snapshot
        .volumes
        .iter()
        .find(|known| known.name == volume)
        .and_then(|known| known.methods.first().copied())
        .unwrap_or(Method::Passphrase);
    model.dialog = Some(crate::menu::model::Dialog::Unlock {
        volume: volume.to_owned(),
        method,
        tries_left: None,
        secret: zbm_platform::secret::Secret::new(),
    });
}

/// The ways to import `pool`.
fn import_choices(model: &mut Model, pool: &zbm_types::PoolName) {
    let choices = vec![
        Choice {
            label: "Import with this machine's host ID".to_owned(),
            then: Then::Act(Action::Import(pool.clone(), ImportPolicy::Strict)),
        },
        Choice {
            label: "Import with the host ID it was last imported with".to_owned(),
            then: Then::Act(Action::Import(pool.clone(), ImportPolicy::Hostid)),
        },
        Choice {
            label: "Force the import (another host may be using it)".to_owned(),
            then: dialogs::force_import(pool),
        },
    ];
    model.dialog = Some(dialogs::choose(
        &format!("Import {pool}"),
        Vec::new(),
        choices,
    ));
}

/// The recovery tools.
fn recovery(model: &mut Model, intent: Intent) -> Vec<Action> {
    if intent != Intent::Primary {
        return Vec::new();
    }
    let Some((_, tool)) = model
        .recovery
        .selected_item()
        .and_then(|index| RECOVERY.get(index))
    else {
        return Vec::new();
    };
    match tool {
        RecoveryTool::Shell => vec![Action::Shell],
        RecoveryTool::Chroot => model
            .selected_environment()
            .map(|environment| vec![Action::Chroot(environment.name.to_string())])
            .unwrap_or_default(),
        RecoveryTool::Log => {
            model.screen = Screen::Log;
            Vec::new()
        }
        RecoveryTool::Pools => {
            model.screen = Screen::Pools;
            Vec::new()
        }
        RecoveryTool::FirmwareSetup => vec![Action::FirmwareSetup],
        RecoveryTool::Reboot => vec![Action::Reboot],
        RecoveryTool::PowerOff => vec![Action::PowerOff],
    }
}
