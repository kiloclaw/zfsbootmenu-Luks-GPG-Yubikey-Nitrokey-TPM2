//! The intents of the main, kernels, pools and recovery views.

use zbm_types::{ImportPolicy, PoolName, Untrusted};

use super::dialogs;
use crate::menu::keymap::{Context, Intent, Tab};
use crate::menu::model::{Choice, Editor, ListState, Model, RECOVERY, RecoveryTool, Screen, Then};
use crate::menu::world::{Access, Action, Environment, Outcome, Verdict, Volume};
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
        Intent::ReadWrite => return vec![read_write(&name.pool())],
        Intent::Chroot => return vec![Action::Chroot(name.into())],
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

/// Pools and volumes. A volume opens through the machine, which asks for
/// what opens it: the unlock dialog opens then. While the machine works,
/// as between two of its questions, a volume does not open again.
fn pools(model: &mut Model, intent: Intent) -> Vec<Action> {
    if let Some(volume) = model.selected_volume() {
        if !matches!(intent, Intent::Primary | Intent::Unlock) || model.working.is_some() {
            return Vec::new();
        }
        return vec![open(volume)];
    }
    let Some(pool) = model.selected_pool().cloned() else {
        return Vec::new();
    };
    match intent {
        Intent::Primary => dialogs::pool_details(model, &pool),
        Intent::Import => import_choices(model, &pool),
        Intent::ReadWrite => return vec![read_write(&pool)],
        Intent::Rewind if !has_checkpoint(model, &pool) => {
            model.notification = Some(Outcome::Warning(text!(
                "pool-no-checkpoint",
                pool = pool.to_string()
            )));
        }
        Intent::Rewind => {
            let lines = vec![
                text!("rewind", pool = pool.to_string()),
                text!("rewind.discarded"),
            ];
            model.dialog = Some(dialogs::confirm(dialogs::rewind(&pool, lines)));
        }
        _ => {}
    }
    Vec::new()
}

/// Opening `volume`: its device, as the mapping it was opened as before,
/// when its name is one, else its default name.
fn open(volume: &Volume) -> Action {
    let device = volume.device.get().clone();
    let name = volume.name.get();
    Action::Open {
        name: (*name != device).then(|| name.parse().ok()).flatten(),
        device,
    }
}

/// Whether the snapshot says `pool` has a checkpoint to rewind to.
fn has_checkpoint(model: &Model, pool: &PoolName) -> bool {
    model
        .snapshot
        .pools
        .iter()
        .any(|known| &known.name == pool && known.checkpoint)
}

/// The read/write import of `pool`.
fn read_write(pool: &PoolName) -> Action {
    dialogs::import(pool, Access::ReadWrite, ImportPolicy::Strict)
}

/// The ways to import `pool`: imported, with the other access, and rewound
/// to its checkpoint when it has one; not imported, as this host, as the
/// host that last imported it, or forced.
fn import_choices(model: &mut Model, pool: &PoolName) {
    let known = model
        .snapshot
        .pools
        .iter()
        .find(|known| &known.name == pool);
    let choices = match known {
        Some(known) if known.imported => {
            let mut choices = vec![dialogs::toggle(pool, known.writable)];
            if known.checkpoint {
                let lines = vec![
                    text!("rewind", pool = pool.to_string()),
                    text!("rewind.discarded"),
                ];
                choices.push(Choice {
                    label: text!("pool-rewind"),
                    then: dialogs::rewind(pool, lines),
                });
            }
            choices
        }
        _ => vec![
            Choice {
                label: text!("pool-import-this-host"),
                then: Then::Act(dialogs::import(
                    pool,
                    Access::ReadOnly,
                    ImportPolicy::Strict,
                )),
            },
            Choice {
                label: text!("pool-import-last-host"),
                then: Then::Act(dialogs::import(
                    pool,
                    Access::ReadOnly,
                    ImportPolicy::Hostid,
                )),
            },
            Choice {
                label: text!("pool-force-import"),
                then: dialogs::force_import(pool),
            },
        ],
    };
    let title = text!("import-title", pool = pool.to_string());
    dialogs::choose_offered(model, &title, Vec::new(), choices);
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
            .map(|environment| vec![Action::Chroot(environment.name.clone().into())])
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
