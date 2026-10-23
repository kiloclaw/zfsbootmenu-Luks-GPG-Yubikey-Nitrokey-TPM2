//! The differences view: what changed since a snapshot, the paths marked to
//! restore, and the list with its filter.

use zbm_types::SnapshotName;

use super::{ListState, refilter};
use crate::menu::world::Change;

/// The differences between a snapshot and the live filesystem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffView {
    /// The snapshot.
    pub from: SnapshotName,
    /// The other snapshot; `None`: the live filesystem.
    pub to: Option<SnapshotName>,
    /// What changed.
    pub changes: Vec<Change>,
    /// The list of changes, by path.
    pub list: ListState,
    /// The paths marked to restore.
    pub marked: Vec<String>,
}

impl DiffView {
    /// The view of `changes` since `from`.
    pub fn new(from: SnapshotName, to: Option<SnapshotName>, changes: Vec<Change>) -> DiffView {
        let mut view = DiffView {
            from,
            to,
            changes,
            list: ListState::default(),
            marked: Vec::new(),
        };
        view.refilter();
        view
    }

    /// Recomputes the list after the changes or the filter changed.
    pub fn refilter(&mut self) {
        let paths: Vec<&str> = self.changes.iter().map(|c| c.path.as_str()).collect();
        refilter(&mut self.list, paths);
        let paths: Vec<&String> = self.changes.iter().map(|c| &c.path).collect();
        self.marked.retain(|path| paths.contains(&path));
    }

    /// The selected change.
    pub fn selected(&self) -> Option<&Change> {
        self.changes.get(self.list.selected_item()?)
    }

    /// Marks the selected path, or unmarks it.
    pub fn toggle_mark(&mut self) {
        let Some(path) = self.selected().map(|change| change.path.clone()) else {
            return;
        };
        if let Some(index) = self.marked.iter().position(|marked| *marked == path) {
            self.marked.remove(index);
        } else {
            self.marked.push(path);
        }
    }

    /// The paths to restore: the marked ones the snapshot has, else the
    /// selected one if the snapshot has it.
    pub fn to_restore(&self) -> Vec<String> {
        let restorable = |path: &String| {
            self.changes
                .iter()
                .any(|change| &change.path == path && change.is_restorable())
        };
        if self.marked.is_empty() {
            return self
                .selected()
                .filter(|change| change.is_restorable())
                .map(|change| vec![change.path.clone()])
                .unwrap_or_default();
        }
        self.marked
            .iter()
            .filter(|path| restorable(path))
            .cloned()
            .collect()
    }
}
