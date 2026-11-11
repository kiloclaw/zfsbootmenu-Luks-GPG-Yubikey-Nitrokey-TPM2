//! The intents of the main, kernels, pools and recovery views.

use zbm_types::{ImportPolicy, Untrusted};

use super::action_of::action_of;
use super::dialogs;
use crate::menu::keymap::{Context, Intent, Map, Tab};
use crate::menu::model::{
    Choice, Column, Editor, ListState, Model, Screen, Then, own_command_line,
};
use crate::menu::world::{Access, Action, Environment, Origin, Outcome, Pool, Verdict, Volume};
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
    let edited = line != *own_command_line(environment);
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
            save: false,
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
        Tab::Boot => model.open_boot_tab(),
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

/// What `intent` does on the main view, as [`action_of`] says.
fn main(model: &mut Model, intent: Intent) -> Vec<Action> {
    match action_of(model, Context::Main, intent) {
        Some(then) => send(model, then),
        None => Vec::new(),
    }
}

/// Follows `then`, which a key or the command line chose: a boot is
/// [`guarded`].
pub(super) fn send(model: &mut Model, then: Then) -> Vec<Action> {
    match then {
        Then::Act(boot @ Action::Boot { .. }) => guarded(model, boot),
        then => dialogs::follow(model, then),
    }
}

/// Drops the command line edited for the boots of `environment`, which
/// boots with its own again, and says so; nothing when none was edited,
/// as the command line offers it only then: no key does what no entry
/// does (rule 1 of decision 18), and nothing says a change that was not
/// made.
pub(super) fn revert(model: &mut Model, environment: &zbm_types::DatasetName) {
    let before = model.edited.len();
    model.edited.retain(|(edited, _)| edited != environment);
    if model.edited.len() == before {
        return;
    }
    model.notification = Some(Outcome::Done(text!(
        "command-line-reverted",
        environment = environment.to_string()
    )));
}

/// Opens the command line of `environment` to save it for every boot: the
/// editor, from the line it boots with now, edited for this boot or not,
/// whose `Enter` shows the dry run of the save (the workspace's alone).
pub(super) fn save_command_line(
    model: &mut Model,
    environment: zbm_types::DatasetName,
) -> Vec<Action> {
    let actions = focus(model, environment, Screen::CommandLine);
    if let Some(editor) = &mut model.editor {
        editor.save = true;
    }
    actions
}

/// The machine is asked for `action`: a command line saved or inherited
/// is kept until the machine shows it done ([`saved`]).
pub(super) fn sent(model: &mut Model, action: &Action) {
    if save_of(action).is_some() {
        model.saving = Some(action.clone());
    }
}

/// A change is done. When the machine's snapshot shows the command line
/// the menu asked it to save, the line edited for this boot of its
/// environment goes, and the save editor of that line, open under its
/// dry run while the machine saved it, closes. Another change done before
/// it leaves both, and so does a save that failed: its editor keeps the
/// line typed, and the edit for this boot stays.
pub(super) fn saved(model: &mut Model) {
    let environments = &model.snapshot.environments;
    let Some(save) = model.saving.take_if(|save| is_shown(environments, save)) else {
        return;
    };
    let Some((environment, line)) = save_of(&save) else {
        return;
    };
    model.edited.retain(|(edited, _)| edited != environment);
    let is_its_editor = model.editor.as_ref().is_some_and(|editor| {
        editor.save && editor.environment == *environment && editor.text == line
    });
    if is_its_editor && model.dialog.is_none() && model.screen == Screen::CommandLine {
        model.editor = None;
        model.screen = Screen::Main;
    }
}

/// The environment of a command line saved or inherited, and the line:
/// empty for an inherit.
fn save_of(action: &Action) -> Option<(&zbm_types::DatasetName, &str)> {
    match action {
        Action::SetCommandLine(environment, line) => Some((environment, line.get().as_str())),
        Action::InheritCommandLine(environment) => Some((environment, "")),
        _ => None,
    }
}

/// Whether `environments` show `save` done: its line set on its
/// environment, or, for an empty line or an inherit, none set there.
fn is_shown(environments: &[Environment], save: &Action) -> bool {
    let Some((environment, line)) = save_of(save) else {
        return false;
    };
    let is_done = |known: &Environment| {
        let is_local = known.command_line.origin == Origin::Local;
        if line.is_empty() {
            !is_local
        } else {
            is_local && known.command_line.value.get() == line
        }
    };
    (environments.iter())
        .filter(|known| known.name == *environment)
        .any(is_done)
}

/// The kernels of the focused environment, as [`action_of`] says.
fn kernels(model: &mut Model, intent: Intent) -> Vec<Action> {
    match action_of(model, Context::Kernels, intent) {
        Some(then) => send(model, then),
        None => Vec::new(),
    }
}

/// Opens the command line of `environment` for a boot with `kernel`.
pub(super) fn edit_for(
    model: &mut Model,
    environment: zbm_types::DatasetName,
    kernel: Untrusted<String>,
) -> Vec<Action> {
    let actions = focus(model, environment, Screen::CommandLine);
    if let Some(editor) = &mut model.editor {
        editor.kernel = Some(kernel);
    }
    actions
}

/// Pools and volumes. A volume opens through the machine, which asks for
/// what opens it: the unlock dialog opens then. While the machine works,
/// as between two of its questions, a volume does not open again. In the
/// workspace's tree, `Enter` on a device nothing is below asks for the
/// kernel's messages about it, by the path it has or had, which show below
/// the tree; on a vdev, as on its pool, it opens the pool's details, and
/// the other keys of a vdev or a device act on its pool.
fn pools(model: &mut Model, intent: Intent) -> Vec<Action> {
    if let Some(then) = action_of(model, Context::Pools, intent) {
        return send(model, then);
    }
    if model.selected_volume().is_some() {
        return Vec::new();
    }
    let Some(pool) = model.selected_pool().cloned() else {
        return Vec::new();
    };
    match intent {
        Intent::Primary => dialogs::pool_details(model, &pool),
        Intent::Import => import_choices(model, &pool),
        Intent::ReadWrite => shadowed(model, &pool),
        Intent::Rewind => {
            model.notification = Some(Outcome::Warning(text!(
                "pool-no-checkpoint",
                pool = pool.name.to_string()
            )));
        }
        _ => {}
    }
    Vec::new()
}

/// What the rewind of `pool` says it discards.
pub(super) fn rewind_lines(pool: &Pool) -> Vec<String> {
    vec![
        text!("rewind", pool = pool.name.to_string()),
        text!("rewind.discarded"),
    ]
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
pub(super) fn open(volume: &Volume) -> Action {
    let device = volume.device.get().clone();
    let name = volume.name.get();
    Action::Open {
        name: (*name != device).then(|| name.parse().ok()).flatten(),
        device,
    }
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
            choices.push(Choice {
                label: text!("pool-rewind"),
                then: dialogs::rewind(model.map, named, rewind_lines(pool)),
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

/// The recovery tools, as [`action_of`] says.
fn recovery(model: &mut Model, intent: Intent) -> Vec<Action> {
    match action_of(model, Context::Recovery, intent) {
        Some(then) => send(model, then),
        None => Vec::new(),
    }
}
