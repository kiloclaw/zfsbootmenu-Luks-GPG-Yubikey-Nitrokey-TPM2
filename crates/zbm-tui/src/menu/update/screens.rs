//! The intents of the main, kernels, pools and recovery views.

use zbm_types::{ImportPolicy, Untrusted};

use super::dialogs;
use crate::menu::keymap::{Context, Intent, Tab};
use crate::menu::model::{Choice, Editor, ListState, Model, RECOVERY, RecoveryTool, Screen, Then};
use crate::menu::world::{Action, Environment, Method, Outcome, Verdict};
use crate::text::text;

/// What `intent` does on the current screen.
pub(super) fn intent(model: &mut Model, intent: Intent) -> Vec<Action> {
    match model.screen {
        Screen::Main => main(model, intent),
        Screen::Kernels => kernels(model, intent),
        Screen::Snapshots => super::snapshots::intent(model, intent),
        Screen::Pools => pools(model, intent),
        Screen::Recovery => recovery(model, intent),
        Screen::Diff => super::diff::intent(model, intent),
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
        Context::Diff | Context::Packages => Screen::Diff,
        _ => Screen::Main,
    }
}

/// The boot of `environment` with `kernel`, and its command line for this
/// boot.
pub(super) fn boot(
    model: &Model,
    environment: &Environment,
    kernel: Option<Untrusted<String>>,
) -> Action {
    let line = model.command_line(environment);
    let edited = line != environment.command_line.value;
    Action::Boot {
        environment: environment.name.clone(),
        kernel,
        command_line: edited.then_some(line),
        countdown: false,
    }
}

/// A boot the user asked for: done at once, unless a check of the
/// environment failed; then an action sheet says why it may not boot and
/// offers to boot anyway (`docs/dev/menu-design/boot-health.md`).
pub(super) fn guarded(model: &mut Model, action: Action) -> Vec<Action> {
    let Action::Boot { environment, .. } = &action else {
        return vec![action];
    };
    let failures: Vec<String> = model
        .snapshot
        .environments
        .iter()
        .filter(|e| &e.name == environment)
        .flat_map(|e| e.checks.iter())
        .filter(|check| check.verdict == Verdict::Failure)
        .map(|check| {
            let detail = crate::menu::format::capitalised(&check.detail);
            match check.fix.as_deref() {
                Some(fix) => format!("{detail}. {}.", crate::menu::format::capitalised(fix)),
                None => format!("{detail}."),
            }
        })
        .collect();
    if failures.is_empty() {
        return vec![action];
    }
    let title = text!("may-not-boot.title", environment = environment.to_string());
    let choice = Choice {
        label: text!("may-not-boot.anyway"),
        then: Then::Act(action),
    };
    model.dialog = Some(dialogs::choose(&title, failures, vec![choice]));
    Vec::new()
}

/// Opens `screen` on `environment`: its snapshots, kernels or command line.
pub(super) fn focus(model: &mut Model, environment: zbm_types::DatasetName, screen: Screen) {
    model.focus = Some(environment);
    model.snapshots = ListState::default();
    model.kernels = ListState::default();
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
            text: model.command_line(&environment).into_inner(),
            environment: environment.name,
            kernel: None,
        });
    }
    model.screen = screen;
}

/// Opens the workspace's `tab`: the snapshots are those of the environment
/// the snapshots or kernels show, else of the selected one; without one
/// the tab stays.
pub(super) fn open_tab(model: &mut Model, tab: Tab) {
    match tab {
        Tab::Boot => {
            model.screen = Screen::Main;
            model.marked.clear();
            model.diff = None;
        }
        Tab::Snapshots => {
            let shown = model
                .focus
                .clone()
                .filter(|_| matches!(model.screen, Screen::Kernels | Screen::CommandLine));
            let environment = shown.or_else(|| {
                model
                    .selected_environment()
                    .map(|environment| environment.name.clone())
            });
            if let Some(name) = environment
                && model.tab() != Tab::Snapshots
            {
                focus(model, name, Screen::Snapshots);
            }
        }
        Tab::Pools => model.screen = Screen::Pools,
        Tab::Log => {
            model.screen = Screen::Log;
            model.scroll = 0;
        }
    }
}

/// The main view.
fn main(model: &mut Model, intent: Intent) -> Vec<Action> {
    let Some(environment) = model.selected_environment().cloned() else {
        return Vec::new();
    };
    let name = environment.name.clone();
    match intent {
        Intent::Primary | Intent::Boot => {
            let action = boot(model, &environment, None);
            return guarded(model, action);
        }
        Intent::Kernels => focus(model, name, Screen::Kernels),
        Intent::Snapshots => focus(model, name, Screen::Snapshots),
        Intent::Edit => focus(model, name, Screen::CommandLine),
        Intent::SetDefault => return vec![Action::SetDefaultEnvironment(name)],
        Intent::Pools => model.screen = Screen::Pools,
        Intent::ReadWrite => return vec![Action::ImportReadWrite(name.pool())],
        Intent::Chroot => return vec![Action::Chroot(name.to_string())],
        Intent::NewSnapshot => {
            let then = super::snapshots::new_snapshot(model, name);
            return dialogs::follow(model, then);
        }
        Intent::Shell => return vec![Action::Shell],
        Intent::RevertCommandLine => {
            model.edited.retain(|(edited, _)| *edited != name);
            model.notification = Some(Outcome::Done(text!(
                "command-line-reverted",
                environment = name.to_string()
            )));
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
        Intent::Primary | Intent::Boot => {
            let action = boot(model, &environment, Some(kernel.version));
            guarded(model, action)
        }
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
                title: text!("rewind.title", pool = pool.to_string()),
                lines: vec![
                    text!("rewind", pool = pool.to_string()),
                    text!("rewind.discarded"),
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
        .find(|known| known.name.get() == volume)
        .and_then(|known| known.methods.first().copied())
        .unwrap_or(Method::Passphrase);
    let prompt = crate::menu::model::Prompt::new(volume.to_owned(), method, None, None);
    model.dialog = Some(crate::menu::model::Dialog::Unlock(prompt));
}

/// The ways to import `pool`.
fn import_choices(model: &mut Model, pool: &zbm_types::PoolName) {
    let choices = vec![
        Choice {
            label: text!("pool-import-this-host"),
            then: Then::Act(Action::Import(pool.clone(), ImportPolicy::Strict)),
        },
        Choice {
            label: text!("pool-import-last-host"),
            then: Then::Act(Action::Import(pool.clone(), ImportPolicy::Hostid)),
        },
        Choice {
            label: text!("pool-force-import"),
            then: dialogs::force_import(pool),
        },
    ];
    model.dialog = Some(dialogs::choose(
        &text!("import-title", pool = pool.to_string()),
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
            model.scroll = 0;
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
