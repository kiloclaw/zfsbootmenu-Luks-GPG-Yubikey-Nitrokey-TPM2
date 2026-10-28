//! The differences view: what changed since a snapshot, in two segments
//! (the files, the paths marked to restore; the packages), each a list with
//! its filter.

use zbm_types::{SnapshotName, Untrusted};

use super::{ListState, refilter};
use crate::menu::world::{Change, PackageChange, PackageDiff};

/// The segments of the differences view.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Segment {
    /// The changed files.
    #[default]
    Files,
    /// The changed packages.
    Packages,
}

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
    pub marked: Vec<Untrusted<String>>,
    /// The packages that changed; `None` without a package database.
    pub packages: Option<PackageDiff>,
    /// The list of packages, by name.
    pub package_list: ListState,
    /// The segment shown.
    pub segment: Segment,
}

impl DiffView {
    /// The view of `changes` and `packages` since `from`.
    pub fn new(
        (from, to): (SnapshotName, Option<SnapshotName>),
        changes: Vec<Change>,
        packages: Option<PackageDiff>,
    ) -> DiffView {
        let mut view = DiffView {
            from,
            to,
            changes,
            list: ListState::default(),
            marked: Vec::new(),
            packages,
            package_list: ListState::default(),
            segment: Segment::Files,
        };
        view.refilter();
        view
    }

    /// Recomputes the list after the changes or the filter changed.
    pub fn refilter(&mut self) {
        let paths: Vec<&str> = self.changes.iter().map(|c| c.path.get().as_str()).collect();
        refilter(&mut self.list, paths);
        let paths: Vec<&Untrusted<String>> = self.changes.iter().map(|c| &c.path).collect();
        self.marked.retain(|path| paths.contains(&path));
        let packages = self
            .packages
            .as_ref()
            .map_or(&[][..], |p| p.changes.as_slice());
        let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
        refilter(&mut self.package_list, names);
    }

    /// The changed packages; none without a package database.
    pub fn package_changes(&self) -> &[PackageChange] {
        self.packages.as_ref().map_or(&[], |p| p.changes.as_slice())
    }

    /// The selected package.
    pub fn selected_package(&self) -> Option<&PackageChange> {
        self.package_changes()
            .get(self.package_list.selected_item()?)
    }

    /// The list of the segment shown.
    pub fn active_list(&self) -> &ListState {
        match self.segment {
            Segment::Files => &self.list,
            Segment::Packages => &self.package_list,
        }
    }

    /// The list of the segment shown, to change.
    pub fn active_list_mut(&mut self) -> &mut ListState {
        match self.segment {
            Segment::Files => &mut self.list,
            Segment::Packages => &mut self.package_list,
        }
    }

    /// Shows the other segment.
    pub fn switch(&mut self) {
        self.segment = match self.segment {
            Segment::Files => Segment::Packages,
            Segment::Packages => Segment::Files,
        };
    }

    /// Shows the files, the first of the selected package's selected.
    pub fn show_package_files(&mut self) {
        let first = self
            .selected_package()
            .and_then(|package| package.files.first().cloned());
        self.segment = Segment::Files;
        self.list.filter.clear();
        self.refilter();
        let Some(first) = first else {
            return;
        };
        let position = self.list.matches.iter().position(|ranked| {
            self.changes
                .get(ranked.index)
                .is_some_and(|change| *change.path.get() == first)
        });
        if let Some(position) = position {
            self.list.selected = position;
        }
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
    pub fn to_restore(&self) -> Vec<Untrusted<String>> {
        let restorable = |path: &Untrusted<String>| {
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
