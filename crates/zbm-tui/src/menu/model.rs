//! The state of the menu: the snapshot of the machine and everything on the
//! screen. Nothing in it changes but through [`update`](fn@super::update).

use zbm_types::{DatasetName, PoolName, SnapshotName, Untrusted};

mod diff;
mod list;
mod naming;
mod prompt;
mod sort;

pub use self::diff::{DiffView, Segment};
use self::list::keep;
pub use self::list::{ListState, refilter};
pub use self::naming::{Make, Naming, is_name_char, timestamp};
pub use self::prompt::{Asking, Prompt};
pub use self::sort::Sort;

use super::filter::Ranked;
use super::keymap::Context;
use super::theme::Theme;
use super::world::{Action, Environment, Outcome, Snapshot, SnapshotInfo, Timeout};
use crate::text::{self, Id, id};

/// The views (`docs/dev/menu-design/information-architecture.md`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    /// Start-up: its stages, and the dialogs it opens.
    Startup,
    /// The boot environments and the detail pane.
    Main,
    /// The snapshots of an environment.
    Snapshots,
    /// The kernels of an environment.
    Kernels,
    /// The command line editor.
    CommandLine,
    /// Pools and LUKS volumes.
    Pools,
    /// Recovery tools.
    Recovery,
    /// The log.
    Log,
    /// Help for the view it was opened from.
    Help(Context),
    /// What changed since a snapshot.
    Diff,
}

/// A choice in a [`Dialog::Choose`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Choice {
    /// What it does, for people.
    pub label: String,
    /// What it leads to.
    pub then: Then,
}

/// What a choice or a confirmation leads to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Then {
    /// Ask the machine to do this.
    Act(Action),
    /// Confirm first, by typing `name`, then act.
    Confirm {
        /// The action.
        action: Action,
        /// The title of the confirmation.
        title: String,
        /// What will happen and what will be lost.
        lines: Vec<String>,
        /// What has to be typed.
        name: String,
    },
    /// Ask for the name of something new, then act.
    Name(Naming),
    /// Open this screen.
    Open(Screen),
    /// Open the snapshots or kernels of an environment, or its command
    /// line.
    Focus(DatasetName, Screen),
}

/// A dialog over a view; not `Clone`: an unlock dialog holds a secret.
#[derive(Debug, Eq, PartialEq)]
pub enum Dialog {
    /// A volume asks for a secret, or waits for a device.
    Unlock(Prompt),
    /// Start-up asks whether to go on.
    Question(Asking),
    /// A choice among actions.
    Choose {
        /// The title.
        title: String,
        /// What the choice is about.
        lines: Vec<String>,
        /// The choices.
        choices: Vec<Choice>,
        /// The selected choice.
        selected: usize,
    },
    /// A destructive action that needs its target's name typed.
    Confirm {
        /// The title.
        title: String,
        /// What will happen and what will be lost.
        lines: Vec<String>,
        /// What has to be typed.
        name: String,
        /// What was typed.
        typed: String,
        /// The action.
        action: Action,
    },
    /// The name of something new.
    Name(Naming),
    /// What failed, why, and the next steps.
    Error {
        /// What failed.
        what: String,
        /// Why.
        why: Vec<String>,
        /// What can be done next.
        next: Vec<String>,
    },
}

/// The command line editor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Editor {
    /// The environment.
    pub environment: DatasetName,
    /// The kernel, when one was chosen.
    pub kernel: Option<Untrusted<String>>,
    /// The text, as the user edits it.
    pub text: String,
}

/// The command palette.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Palette {
    /// The query.
    pub query: String,
    /// What it matches, best first, as indexes of
    /// [`palette_items`](super::update::palette_items).
    pub matches: Vec<Ranked>,
    /// The selection in `matches`.
    pub selected: usize,
}

/// One entry of the command palette.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteItem {
    /// What the palette shows and matches.
    pub label: String,
    /// What it does.
    pub then: Then,
}

/// The menu.
#[derive(Debug, Eq, PartialEq)]
pub struct Model {
    /// The machine.
    pub snapshot: Snapshot,
    /// How to draw.
    pub theme: Theme,
    /// The view.
    pub screen: Screen,
    /// The dialog over it.
    pub dialog: Option<Dialog>,
    /// The palette, when open.
    pub palette: Option<Palette>,
    /// The command line editor, when open.
    pub editor: Option<Editor>,
    /// The environments of the main view, sorted, defaults first.
    pub environments: ListState,
    /// The snapshots of [`Model::focus`].
    pub snapshots: ListState,
    /// Snapshots marked for a diff.
    pub marked: Vec<SnapshotName>,
    /// The kernels of [`Model::focus`].
    pub kernels: ListState,
    /// The pools and volumes, as one list: pools first.
    pub pools: ListState,
    /// The recovery tools.
    pub recovery: ListState,
    /// The environment the snapshots and kernels views show.
    pub focus: Option<DatasetName>,
    /// The sort order of the snapshots.
    pub sort: Sort,
    /// Command lines edited for this boot, by environment.
    pub edited: Vec<(DatasetName, Untrusted<String>)>,
    /// The countdown, in milliseconds, while it runs.
    pub countdown: Option<u64>,
    /// Whether start-up finished and the main view opened.
    pub started: bool,
    /// The outcome of the last action.
    pub notification: Option<Outcome>,
    /// What start-up is doing that may take a while, until its next event.
    pub progress: Option<String>,
    /// How long the action the machine performs has run, in milliseconds,
    /// while it runs: Escape cancels it.
    pub working: Option<u64>,
    /// Why the menu ends, once it does.
    pub leaving: Option<String>,
    /// The key presses so far, for the measures of the core tasks.
    pub keys: u32,
    /// How many lines help or the log are scrolled down.
    pub scroll: usize,
    /// The differences view, when open.
    pub diff: Option<DiffView>,
    /// The keyboard layout typing is translated into, as an index of the
    /// snapshot's layouts: 0 is the console's own.
    pub layout: usize,
}

impl Model {
    /// The menu for `snapshot`: on the start-up view while start-up runs.
    pub fn new(snapshot: Snapshot, theme: Theme) -> Model {
        let mut model = Model {
            screen: Screen::Startup,
            theme,
            dialog: None,
            palette: None,
            editor: None,
            environments: ListState::default(),
            snapshots: ListState::default(),
            marked: Vec::new(),
            kernels: ListState::default(),
            pools: ListState::default(),
            recovery: ListState::default(),
            focus: None,
            sort: Sort::Name,
            edited: Vec::new(),
            countdown: None,
            started: false,
            notification: None,
            progress: None,
            working: None,
            leaving: None,
            keys: 0,
            scroll: 0,
            diff: None,
            layout: 0,
            snapshot,
        };
        model.refresh();
        model
    }

    /// Gets the model ready for another round of the menu, as the session
    /// runs it after the recovery shell: the menu's own view, without what
    /// ended the last round.
    pub fn begin_round(&mut self) {
        self.leaving = None;
        self.dialog = None;
        self.progress = None;
        self.working = None;
        self.countdown = None;
        if self.started {
            self.screen = Screen::Main;
        }
        // The round's machine opens the main view again, with its own
        // countdown: the start-up view saw start-up end, but counted
        // nothing down, as it knew no timeout.
        self.started = false;
    }

    /// Takes the machine's new `snapshot`, keeping the environment and the
    /// snapshot selected by name wherever the new lists put them.
    pub fn replace_snapshot(&mut self, snapshot: Snapshot) {
        self.refresh_with(Some(snapshot));
    }

    /// Recomputes the lists after a filter or the sort order changed. The
    /// selected environment and snapshot stay selected while they match;
    /// other selections stay in range.
    pub fn refresh(&mut self) {
        self.refresh_with(None);
    }

    /// Sorts the snapshots by `sort`. The selection is read in the old order,
    /// so a rollback takes the snapshot shown selected before.
    pub fn set_sort(&mut self, sort: Sort) {
        let selected = self.selected_names();
        self.sort = sort;
        self.refresh_keeping(None, selected);
    }

    /// The names of the selected environment and snapshot.
    fn selected_names(&self) -> (Option<String>, Option<String>) {
        (
            self.selected_environment().map(|e| e.name.to_string()),
            self.selected_snapshot().map(|s| s.name.name().to_owned()),
        )
    }

    /// [`Model::refresh`], with `new` in place of the snapshot when given.
    fn refresh_with(&mut self, new: Option<Snapshot>) {
        let selected = self.selected_names();
        self.refresh_keeping(new, selected);
    }

    /// [`Model::refresh_with`], keeping the environment and snapshot named.
    fn refresh_keeping(
        &mut self,
        new: Option<Snapshot>,
        selected: (Option<String>, Option<String>),
    ) {
        let (environment, snapshot) = selected;
        if let Some(new) = new {
            self.snapshot = new;
        }
        let names = self.environment_order();
        let labels: Vec<String> = names
            .iter()
            .filter_map(|&index| self.snapshot.environments.get(index))
            .map(|environment| environment.name.to_string())
            .collect();
        refilter(&mut self.environments, labels.iter().map(String::as_str));
        for ranked in &mut self.environments.matches {
            ranked.index = names.get(ranked.index).copied().unwrap_or(ranked.index);
        }
        keep(
            &mut self.environments,
            &self.snapshot.environments,
            environment,
            |e| e.name.to_string(),
        );
        let snapshots: Vec<String> = self
            .focus_snapshots()
            .iter()
            .map(|info| info.name.name().to_owned())
            .collect();
        refilter(&mut self.snapshots, snapshots.iter().map(String::as_str));
        keep(&mut self.snapshots, &snapshots, snapshot, Clone::clone);
        let kernels: Vec<String> = self
            .focused()
            .map(|environment| {
                environment
                    .kernels
                    .iter()
                    .map(|k| k.version.get().clone())
                    .collect()
            })
            .unwrap_or_default();
        refilter(&mut self.kernels, kernels.iter().map(String::as_str));
        let pools = self.pool_rows();
        refilter(&mut self.pools, pools.iter().map(String::as_str));
        let tools: Vec<String> = RECOVERY.iter().map(|(tool, _)| text::get(*tool)).collect();
        refilter(&mut self.recovery, tools.iter().map(String::as_str));
        if let Some(diff) = &mut self.diff {
            diff.refilter();
        }
    }

    /// The indexes of the environments in the order of the main view:
    /// defaults first, then by name.
    fn environment_order(&self) -> Vec<usize> {
        let mut order: Vec<(usize, &Environment)> =
            self.snapshot.environments.iter().enumerate().collect();
        order.sort_by(|(_, a), (_, b)| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
        order.into_iter().map(|(index, _)| index).collect()
    }

    /// Selects the environment `name` in the main view, if it is listed.
    pub fn select_environment(&mut self, name: &DatasetName) {
        let environments = &self.snapshot.environments;
        let found = self.environments.matches.iter().position(|ranked| {
            environments
                .get(ranked.index)
                .is_some_and(|environment| &environment.name == name)
        });
        if let Some(position) = found {
            self.environments.selected = position;
        }
    }

    /// The selected environment of the main view.
    pub fn selected_environment(&self) -> Option<&Environment> {
        self.snapshot
            .environments
            .get(self.environments.selected_item()?)
    }

    /// The environment the snapshots and kernels views show.
    pub fn focused(&self) -> Option<&Environment> {
        let focus = self.focus.as_ref()?;
        self.snapshot
            .environments
            .iter()
            .find(|environment| &environment.name == focus)
    }

    /// The snapshots of the focused environment, in the sort order.
    pub fn focus_snapshots(&self) -> Vec<&SnapshotInfo> {
        let mut snapshots: Vec<&SnapshotInfo> = self
            .focused()
            .map(|environment| environment.snapshots.iter().collect())
            .unwrap_or_default();
        match self.sort {
            Sort::Name => snapshots.sort_by(|a, b| a.name.name().cmp(b.name.name())),
            Sort::Creation => snapshots.sort_by_key(|s| std::cmp::Reverse(s.created)),
            Sort::Size => snapshots.sort_by_key(|s| std::cmp::Reverse(s.used)),
        }
        snapshots
    }

    /// The selected snapshot.
    pub fn selected_snapshot(&self) -> Option<&SnapshotInfo> {
        let snapshots = self.focus_snapshots();
        snapshots.get(self.snapshots.selected_item()?).copied()
    }

    /// The rows of the pools view: each pool, then each volume.
    pub fn pool_rows(&self) -> Vec<String> {
        let pools = self.snapshot.pools.iter().map(|pool| pool.name.to_string());
        let volumes = self
            .snapshot
            .volumes
            .iter()
            .map(|volume| volume.name.get().clone());
        pools.chain(volumes).collect()
    }

    /// The pool of the selected row of the pools view, if it is a pool.
    pub fn selected_pool(&self) -> Option<&PoolName> {
        let row = self.pools.selected_item()?;
        self.snapshot.pools.get(row).map(|pool| &pool.name)
    }

    /// The name of the volume of the selected row of the pools view.
    pub fn selected_volume(&self) -> Option<&str> {
        let row = self
            .pools
            .selected_item()?
            .checked_sub(self.snapshot.pools.len())?;
        self.snapshot
            .volumes
            .get(row)
            .map(|volume| volume.name.get().as_str())
    }

    /// The command line `environment` boots with: the edited one, else its
    /// own.
    pub fn command_line(&self, environment: &Environment) -> Untrusted<String> {
        self.edited
            .iter()
            .find(|(name, _)| *name == environment.name)
            .map_or_else(
                || environment.command_line.value.clone(),
                |(_, line)| line.clone(),
            )
    }

    /// Whether start-up is over: every stage done, failed or skipped.
    pub fn is_started(&self) -> bool {
        self.snapshot.startup.iter().all(|stage| {
            !matches!(
                stage.state,
                super::world::StageState::Pending | super::world::StageState::Running
            )
        })
    }

    /// The key map context of the focus.
    pub fn context(&self) -> Context {
        match &self.dialog {
            Some(Dialog::Unlock(_) | Dialog::Confirm { .. } | Dialog::Name(_)) => {
                return Context::Text;
            }
            Some(Dialog::Choose { .. } | Dialog::Error { .. } | Dialog::Question(_)) => {
                return Context::Choose;
            }
            None => {}
        }
        if self.palette.is_some()
            || self.editor.is_some()
            || self.list().is_some_and(|l| l.filtering)
        {
            return Context::Text;
        }
        match self.screen {
            Screen::Startup => Context::Startup,
            Screen::Main => Context::Main,
            Screen::Snapshots => Context::Snapshots,
            Screen::Kernels => Context::Kernels,
            Screen::CommandLine => Context::Text,
            Screen::Pools => Context::Pools,
            Screen::Recovery => Context::Recovery,
            Screen::Log => Context::Log,
            Screen::Help(_) => Context::Help,
            Screen::Diff => match self.diff.as_ref().map(|diff| diff.segment) {
                Some(Segment::Packages) => Context::Packages,
                _ => Context::Diff,
            },
        }
    }

    /// The list of the current screen.
    pub fn list(&self) -> Option<&ListState> {
        match self.screen {
            Screen::Main => Some(&self.environments),
            Screen::Snapshots => Some(&self.snapshots),
            Screen::Kernels => Some(&self.kernels),
            Screen::Pools => Some(&self.pools),
            Screen::Recovery => Some(&self.recovery),
            Screen::Diff => self.diff.as_ref().map(DiffView::active_list),
            _ => None,
        }
    }

    /// The list of the current screen, to change.
    pub fn list_mut(&mut self) -> Option<&mut ListState> {
        match self.screen {
            Screen::Main => Some(&mut self.environments),
            Screen::Snapshots => Some(&mut self.snapshots),
            Screen::Kernels => Some(&mut self.kernels),
            Screen::Pools => Some(&mut self.pools),
            Screen::Recovery => Some(&mut self.recovery),
            Screen::Diff => self.diff.as_mut().map(DiffView::active_list_mut),
            _ => None,
        }
    }

    /// The keyboard layout typing is translated into: the console's own
    /// until the user chooses another.
    pub fn layout(&self) -> Option<&str> {
        self.snapshot
            .keyboard
            .layouts
            .get(self.layout)
            .map(String::as_str)
    }

    /// The seconds left of the countdown, rounded up.
    pub fn countdown_seconds(&self) -> Option<u64> {
        self.countdown.map(|ms| ms.div_ceil(1000))
    }

    /// The countdown of the timeout, in milliseconds.
    pub fn timeout_ms(&self) -> Option<u64> {
        match self.snapshot.timeout {
            Timeout::Seconds(seconds) => Some(u64::from(seconds) * 1000),
            Timeout::Skip => Some(0),
            Timeout::Wait => None,
        }
    }
}

/// The recovery tools: what the list shows (the title of the message) and
/// what it does.
pub const RECOVERY: &[(Id, RecoveryTool)] = &[
    (id!("tool-shell"), RecoveryTool::Shell),
    (id!("tool-chroot"), RecoveryTool::Chroot),
    (id!("tool-log"), RecoveryTool::Log),
    (id!("tool-pools"), RecoveryTool::Pools),
    (id!("tool-firmware-setup"), RecoveryTool::FirmwareSetup),
    (id!("tool-reboot"), RecoveryTool::Reboot),
    (id!("tool-power-off"), RecoveryTool::PowerOff),
];

/// A recovery tool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryTool {
    /// The recovery shell.
    Shell,
    /// A chroot of the selected environment.
    Chroot,
    /// The log.
    Log,
    /// Pools and volumes.
    Pools,
    /// Firmware setup.
    FirmwareSetup,
    /// Reboot.
    Reboot,
    /// Power off.
    PowerOff,
}

#[cfg(test)]
mod tests;
