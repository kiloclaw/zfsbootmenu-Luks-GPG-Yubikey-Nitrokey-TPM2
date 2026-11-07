//! The intents of the main, kernels, pools and recovery views.

use zbm_types::{ImportPolicy, PoolName, Untrusted};

use super::dialogs;
use crate::menu::keymap::{Context, Intent, Map, Tab};
use crate::menu::model::{
    Choice, Column, Editor, Item, Kind, ListState, Model, Node, RECOVERY, RecoveryTool, Screen,
    Then,
};
use crate::menu::world::{Access, Action, Environment, Outcome, Pool, Verdict, Volume};
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

/// Opens `screen` on `environment`: its snapshots, which the machine is
/// asked for, its kernels or its command line. In the workspace a column
/// keeps its selection, and the marks, while it shows the same environment
/// (ADR 0007, decision 2), and the environments select the one it shows;
/// the kernels select the default kernel until the user moves in them.
pub(super) fn focus(
    model: &mut Model,
    environment: zbm_types::DatasetName,
    screen: Screen,
) -> Vec<Action> {
    let workspace = model.map == Map::Workspace;
    let kept = workspace && model.focus.as_ref() == Some(&environment);
    if workspace {
        model.select_environment(&environment);
    }
    model.focus = Some(environment);
    if !kept {
        model.snapshots = ListState::default();
        model.rank_again();
        model.kernels = ListState::default();
        model.marked.clear();
    }
    model.refresh();
    // The workspace's kernels open on the default kernel until the user
    // moves in them, whatever screen the focus changed on.
    if !kept && (workspace || screen == Screen::Kernels) {
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
    if screen == Screen::Snapshots {
        return super::snapshots::load(model);
    }
    Vec::new()
}

/// Opens the workspace's `tab`: the snapshots are those of the environment
/// the snapshots or kernels show, else of the selected one, which the
/// machine is asked for; without one the tab stays, and during start-up,
/// which has none yet, says when they show. A tab other than Boot opened
/// during start-up says someone is there ([`Model::away`]).
pub(super) fn open_tab(model: &mut Model, tab: Tab) -> Vec<Action> {
    let actions = switch_tab(model, tab);
    if tab == Tab::Snapshots && model.tab() != tab && !model.started {
        let later = text!("snapshots-after-startup");
        model.notification = Some(Outcome::Warning(later));
    }
    mark_away(model);
    actions
}

/// Opens the tab `step` gives from the current one, as [`open_tab`] does;
/// a tab that stays shut, as the Snapshots tab with no environment, is
/// passed over for the one after it.
pub(super) fn cycle_tab(model: &mut Model, step: fn(Tab) -> Tab) -> Vec<Action> {
    let mut tab = model.tab();
    let mut actions = Vec::new();
    for _ in 1..Tab::ALL.len() {
        tab = step(tab);
        actions = switch_tab(model, tab);
        if model.tab() == tab {
            break;
        }
    }
    mark_away(model);
    actions
}

/// Marks someone there when, during start-up, a tab other than Boot is
/// open ([`Model::away`]); the first such tab is kept.
fn mark_away(model: &mut Model) {
    if !model.started && model.tab() != Tab::Boot && model.away.is_none() {
        model.away = Some(model.tab());
    }
}

/// [`open_tab`], but for what it says.
fn switch_tab(model: &mut Model, tab: Tab) -> Vec<Action> {
    match tab {
        Tab::Boot => {
            model.screen = model.boot_view();
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
            if model.tab() == Tab::Snapshots {
                // On the tab, its key reads the snapshots not read.
                if model.column() == Column::NotRead {
                    return super::snapshots::load(model);
                }
            } else if let Some(name) = environment {
                return focus(model, name, Screen::Snapshots);
            }
        }
        Tab::Pools => model.screen = Screen::Pools,
        Tab::Log => {
            model.screen = Screen::Log;
            model.scroll = 0;
        }
    }
    Vec::new()
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
        Intent::Kernels => return focus(model, name, Screen::Kernels),
        Intent::Snapshots => return focus(model, name, Screen::Snapshots),
        Intent::Edit => return focus(model, name, Screen::CommandLine),
        Intent::SetDefault => return vec![Action::SetDefaultEnvironment(name)],
        Intent::Pools => model.screen = Screen::Pools,
        Intent::ReadWrite => return vec![read_write(model, &name.pool())],
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
            let actions = focus(model, name, Screen::CommandLine);
            if let Some(editor) = &mut model.editor {
                editor.kernel = Some(kernel.version);
            }
            actions
        }
        _ => Vec::new(),
    }
}

/// Pools and volumes. A volume opens through the machine, which asks for
/// what opens it: the unlock dialog opens then. While the machine works,
/// as between two of its questions, a volume does not open again. In the
/// workspace's tree, `Enter` on a device nothing is below asks for the
/// kernel's messages about it, by the path it has or had, which show below
/// the tree; on a vdev, as on its pool, it opens the pool's details, and
/// the other keys of a vdev or a device act on its pool.
fn pools(model: &mut Model, intent: Intent) -> Vec<Action> {
    if let Some(volume) = model.selected_volume() {
        if !matches!(intent, Intent::Primary | Intent::Unlock) || model.working.is_some() {
            return Vec::new();
        }
        return vec![open(volume)];
    }
    if intent == Intent::Primary
        && let Some(Node {
            kind: Kind::Device,
            item: Item::Device(_, device),
            ..
        }) = model
            .selected_node()
            .filter(|_| model.map == Map::Workspace)
    {
        let path = Untrusted::new(device.path().to_owned());
        return vec![Action::LoadDeviceLog(path)];
    }
    let Some(pool) = model.selected_pool().cloned() else {
        return Vec::new();
    };
    let named = dialogs::Named::of(model, &pool);
    match intent {
        Intent::Primary => dialogs::pool_details(model, &pool),
        Intent::Import => import_choices(model, &pool),
        Intent::ReadWrite if model.is_shadowed(&pool) => shadowed(model, &pool),
        Intent::ReadWrite => {
            return vec![dialogs::import(
                named,
                Access::ReadWrite,
                ImportPolicy::Strict,
            )];
        }
        // Unknown, as for a pool that is not imported, the machine checks.
        Intent::Rewind if pool.checkpoint == Some(false) => {
            model.notification = Some(Outcome::Warning(text!(
                "pool-no-checkpoint",
                pool = pool.name.to_string()
            )));
        }
        Intent::Rewind => {
            let lines = vec![
                text!("rewind", pool = pool.name.to_string()),
                text!("rewind.discarded"),
            ];
            let then = dialogs::rewind(model.map, named, lines);
            return dialogs::follow(model, then);
        }
        _ => {}
    }
    Vec::new()
}

/// Says that `pool` is not imported while a pool of its name is.
fn shadowed(model: &mut Model, pool: &Pool) {
    model.notification = Some(Outcome::Warning(text!(
        "pool-shadowed",
        pool = pool.name.to_string()
    )));
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

/// The read/write import of `pool`, the imported pool of that name, by its
/// identifier when a pool not imported has the name too.
fn read_write(model: &Model, pool: &PoolName) -> Action {
    let imported = model
        .snapshot
        .pools
        .iter()
        .find(|known| known.imported && &known.name == pool);
    let named = imported.map_or_else(|| pool.into(), |known| dialogs::Named::of(model, known));
    dialogs::import(named, Access::ReadWrite, ImportPolicy::Strict)
}

/// The ways to import `pool`: imported, with the other access, and rewound
/// to its checkpoint when it has one; not imported, as this host, as the
/// host that last imported it, or forced, unless a pool of its name is
/// imported.
fn import_choices(model: &mut Model, pool: &Pool) {
    let named = dialogs::Named::of(model, pool);
    let name = pool.name.to_string();
    let choices = if pool.imported {
        let mut choices = vec![dialogs::toggle(named, pool.writable)];
        if pool.checkpoint == Some(true) {
            let lines = vec![
                text!("rewind", pool = name.as_str()),
                text!("rewind.discarded"),
            ];
            choices.push(Choice {
                label: text!("pool-rewind"),
                then: dialogs::rewind(model.map, named, lines),
            });
        }
        choices
    } else if model.is_shadowed(pool) {
        shadowed(model, pool);
        return;
    } else {
        vec![
            Choice {
                label: text!("pool-import-this-host"),
                then: Then::Act(dialogs::import(
                    named,
                    Access::ReadOnly,
                    ImportPolicy::Strict,
                )),
            },
            Choice {
                label: text!("pool-import-last-host"),
                then: Then::Act(dialogs::import(
                    named,
                    Access::ReadOnly,
                    ImportPolicy::Hostid,
                )),
            },
            Choice {
                label: text!("pool-force-import"),
                then: dialogs::force_import(model.map, named),
            },
        ]
    };
    let title = text!("import-title", pool = name.as_str());
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
