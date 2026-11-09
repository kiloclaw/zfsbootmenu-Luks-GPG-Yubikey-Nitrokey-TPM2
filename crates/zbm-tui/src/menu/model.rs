//! The state of the menu: the snapshot of the machine and everything on the
//! screen. Nothing in it changes but through [`update`](fn@super::update).

use std::collections::{BTreeMap, BTreeSet};

use zbm_types::{DatasetName, SnapshotName, Untrusted};

mod columns;
mod devices;
mod diff;
mod list;
mod naming;
mod palette;
mod places;
mod pools;
mod prompt;
mod recovery;
mod review;
mod snapshots;
mod sort;
mod timeline;
mod typing;

pub(crate) use self::devices::{Item, Kind, Node, class_heading, vdev_kind};
pub use self::diff::{DiffView, Segment};
use self::list::keep;
pub use self::list::{ListState, refilter};
pub use self::naming::{Make, Naming, is_name_char, timestamp};
pub use self::palette::{Palette, PaletteItem};
pub(crate) use self::pools::{disk_name, host_text, pool_mode, pool_name};
pub use self::prompt::{Asking, Prompt};
pub(crate) use self::recovery::recovery_tools;
pub use self::recovery::{RECOVERY, RecoveryTool};
pub use self::review::Reviewing;
pub(crate) use self::review::{DryRun, Focus, Lost};
pub(crate) use self::snapshots::Column;
pub use self::sort::Sort;
use self::timeline::Folds;
pub(crate) use self::timeline::{Group, Row, Timeline};
pub use self::typing::TypedOn;

use super::keymap::{Context, Intent, Map, Tab};
use super::theme::Theme;
use super::world::{Action, DeviceLog, Environment, History, Outcome, Snapshot, Timeout};

/// What a refresh keeps selected: the environment and the snapshot named,
/// the row of the pools view ([`Model::selected_row`]), and the kernel
/// named.
type Selected = (
    Option<String>,
    Option<String>,
    Option<pools::Row>,
    Option<String>,
);

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
    /// Open the review page of this change, under the workspace: what it
    /// loses, keeps and allows, then its target's name typed; then act.
    Review(Action),
    /// Ask for the name of something new, then act.
    Name(Naming),
    /// Ask for the name of a clone of this snapshot, then boot it: the
    /// palette's entries, whose naming is made only once one is chosen.
    BootClone(SnapshotName),
    /// Open this screen.
    Open(Screen),
    /// Open the snapshots or kernels of an environment, or its command
    /// line.
    Focus(DatasetName, Screen),
    /// Do what a key of the view under the palette does: an action only a
    /// character or a chord reaches (rule 1 of decision 18).
    Intent(Intent),
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
    /// The review page of a change, which fills the view; boxed, as its
    /// plan is large.
    Review(Box<Reviewing>),
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
    /// Whether a change being chosen shows its plan first.
    pub(crate) dry_run: DryRun,
    /// The command line editor, when open.
    pub editor: Option<Editor>,
    /// The environments of the main view, sorted, defaults first.
    pub environments: ListState,
    /// The snapshots of [`Model::focus`].
    pub snapshots: ListState,
    /// The environment whose snapshots the menu asked for, and the machine
    /// has not sent yet: its column says it reads them.
    pub reading: Option<DatasetName>,
    /// The environment whose snapshots the machine could not read: a
    /// change does not ask for them again, as that would fail again;
    /// opening their column does.
    pub unread: Option<DatasetName>,
    /// How many times the lists of snapshots read changed: their load,
    /// and their sort.
    generation: u64,
    /// What the snapshots' matches were ranked from, which a refresh ranks
    /// again only when it changed.
    ranked: Option<snapshots::Ranked>,
    /// The snapshots as the workspace's timeline groups them, when it does.
    timeline: Option<Timeline>,
    /// The groups of the timeline the person folded or opened.
    folds: Folds,
    /// The snapshots marked, for a diff, all of the focused environment:
    /// a set, as many as are marked; none that a list read again no longer
    /// has.
    pub marked: BTreeSet<SnapshotName>,
    /// The place of each snapshot read in its list as the machine sent it,
    /// newest first: the order ZFS took them in, which tells two taken in
    /// the same second apart where the menu's sort does not.
    ages: BTreeMap<SnapshotName, usize>,
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
    /// The tab the person left the Boot tab for during start-up, outside a
    /// dialog, under the workspace: someone is there, looking at something,
    /// and the menu opens without its countdown, on the tab they are on.
    /// Keys typed into a dialog, such as a passphrase, do not count.
    pub away: Option<Tab>,
    /// Whether the menu asked the machine to scan the pools, which it does
    /// once the main view is drawn and no countdown runs, so that neither
    /// the first frame nor the boot the countdown ends with waits for it.
    pub scanned: bool,
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
    /// The kernel's messages about a device of a pool, which the
    /// workspace's Pools tab shows below the pools until Back.
    pub device_log: Option<DeviceLog>,
    /// The boot history, once the machine sent it, which the workspace's
    /// Log tab and inspector show.
    pub history: Option<History>,
    /// Whether the menu asked for the boot history, and the machine has not
    /// answered yet.
    pub reading_history: bool,
    /// The keyboard layout `Ctrl-K` chose to translate typing into, as an
    /// index of the snapshot's layouts: `None` until it chose one.
    pub layout: Option<usize>,
    /// Where the keys come from, as the input says; `None` when it does
    /// not.
    pub typed_on: Option<TypedOn>,
    /// The key map: the workspace's, or the classic one.
    pub map: Map,
    /// The milliseconds the ticks counted since the snapshot's time: the
    /// tab bar's clock is that time and these.
    pub ticked: u64,
}

impl Model {
    /// The menu for `snapshot`: on the start-up view while start-up runs.
    pub fn new(mut snapshot: Snapshot, theme: Theme) -> Model {
        snapshots::sort_lists(&mut snapshot.loaded, Sort::Name);
        let mut model = Model {
            screen: Screen::Startup,
            theme,
            dialog: None,
            palette: None,
            dry_run: DryRun::Off,
            editor: None,
            environments: ListState::default(),
            snapshots: ListState::default(),
            reading: None,
            unread: None,
            generation: 0,
            ranked: None,
            timeline: None,
            folds: Folds::default(),
            marked: BTreeSet::new(),
            ages: BTreeMap::new(),
            kernels: ListState::default(),
            pools: ListState::default(),
            recovery: ListState::default(),
            focus: None,
            sort: Sort::Name,
            edited: Vec::new(),
            countdown: None,
            started: false,
            away: None,
            scanned: false,
            notification: None,
            progress: None,
            working: None,
            leaving: None,
            keys: 0,
            scroll: 0,
            diff: None,
            device_log: None,
            history: None,
            reading_history: false,
            layout: None,
            typed_on: None,
            map: Map::default(),
            ticked: 0,
            snapshot,
        };
        model.refresh();
        model
    }

    /// Gets the model ready for another round of the menu, as the session
    /// runs it after the recovery shell: the menu's own view, without what
    /// ended the last round. Someone who left the Boot tab during start-up
    /// stays on their tab, whose start-up view becomes the main view. The
    /// boot history asked for and not answered is asked for again, of this
    /// round's machine.
    pub fn begin_round(&mut self) {
        self.leaving = None;
        self.dialog = None;
        self.progress = None;
        self.working = None;
        self.reading = None;
        self.countdown = None;
        if self.started {
            self.screen = if self.away.is_some() {
                self.started_view()
            } else {
                Screen::Main
            };
        }
        self.reading_history = false;
        // The round's machine opens the main view again, with its own
        // countdown and scan: the start-up view only noted that start-up
        // ended.
        self.started = false;
        self.scanned = false;
    }

    /// The view once start-up is over: the main view. Under the workspace
    /// the person stays on the tab they are on; the Boot tab's start-up
    /// view, and its help, become those of its main view.
    pub(crate) fn started_view(&self) -> Screen {
        match (self.map, self.screen) {
            (Map::Workspace, Screen::Help(Context::Startup)) => Screen::Help(Context::Main),
            (Map::Workspace, Screen::Startup) | (Map::Classic, _) => Screen::Main,
            (Map::Workspace, screen) => screen,
        }
    }

    /// Takes the machine's new `snapshot`, keeping the environment and the
    /// snapshot selected by name, and the pools view's row by what it
    /// shows, wherever the new lists put them.
    pub fn replace_snapshot(&mut self, snapshot: Snapshot) {
        self.refresh_with(Some(snapshot));
    }

    /// Takes the `pools` a scan found in place of the snapshot's, keeping
    /// the selections as [`Model::replace_snapshot`] does.
    pub(crate) fn replace_pools(&mut self, pools: Vec<super::world::Pool>) {
        let selected = self.selected_names();
        self.snapshot.pools = pools;
        self.refresh_keeping(None, selected);
    }

    /// Recomputes the lists after a filter or the sort order changed. The
    /// selected environment and snapshot stay selected while they match;
    /// other selections stay in range.
    pub fn refresh(&mut self) {
        self.refresh_with(None);
    }

    /// The time, in seconds since the epoch: the snapshot's and the ticks
    /// counted since, as the tab bar's clock and the timeline's days say.
    pub(crate) fn clock(&self) -> u64 {
        self.snapshot.now.saturating_add(self.ticked / 1000)
    }

    /// Sorts the snapshots by `sort`. The selection is read in the old order,
    /// so a rollback takes the snapshot shown selected before.
    pub fn set_sort(&mut self, sort: Sort) {
        let selected = self.selected_names();
        self.sort = sort;
        snapshots::sort_lists(&mut self.snapshot.loaded, sort);
        self.generation = self.generation.wrapping_add(1);
        self.refresh_keeping(None, selected);
    }

    /// The names of the selected environment, snapshot and kernel, and the
    /// selected row of the pools view.
    fn selected_names(&self) -> Selected {
        (
            self.selected_environment().map(|e| e.name.to_string()),
            self.snapshot_at_selection()
                .map(|s| s.name.name().to_owned()),
            self.selected_row(),
            self.selected_kernel().map(|k| k.version.get().clone()),
        )
    }

    /// [`Model::refresh`], with `new` in place of the snapshot when given.
    fn refresh_with(&mut self, new: Option<Snapshot>) {
        let selected = self.selected_names();
        self.refresh_keeping(new, selected);
    }

    /// [`Model::refresh_with`], keeping the environment and snapshot named,
    /// the row of the pools view, and in the workspace the kernel named.
    fn refresh_keeping(&mut self, new: Option<Snapshot>, selected: Selected) {
        let (environment, snapshot, row, kernel) = selected;
        if let Some(mut new) = new {
            let ages = snapshots::ages(&new.loaded);
            snapshots::sort_lists(&mut new.loaded, self.sort);
            let relisted = new.loaded != self.snapshot.loaded;
            if relisted {
                self.generation = self.generation.wrapping_add(1);
            }
            if new.now != self.snapshot.now {
                self.ticked = 0;
            }
            self.snapshot = new;
            if relisted {
                self.ages = ages;
                self.forget_unlisted_marks();
            }
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
        self.rank_snapshots(snapshot);
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
        self.keep_kernel(kernel);
        self.refilter_pools(row);
        recovery::refresh(&mut self.recovery, &self.snapshot);
        if let Some(diff) = &mut self.diff {
            diff.refilter();
        }
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

#[cfg(test)]
mod tests;
