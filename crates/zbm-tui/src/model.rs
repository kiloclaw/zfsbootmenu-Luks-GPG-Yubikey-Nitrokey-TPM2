//! The state of the menu.

use zbm_app::{Encryption, Snapshot};
use zbm_types::DatasetName;

use crate::filter;

/// One boot environment in the list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The filesystem.
    pub name: DatasetName,
    /// Whether it is the default of its pool.
    pub is_default: bool,
    /// Its encryption.
    pub encryption: Encryption,
}

/// An entry that the filter text matches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Match {
    /// The index of the entry in [`Model::entries`].
    pub entry: usize,
    /// The positions, in characters, of the name that the filter matched.
    pub positions: Vec<usize>,
}

/// Everything the menu shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Model {
    /// The boot environments: defaults first, then by name.
    pub(crate) entries: Vec<Entry>,
    /// What the user typed to filter the list.
    pub(crate) filter: String,
    /// The entries the filter matches, best first.
    pub(crate) matches: Vec<Match>,
    /// The index in `matches` of the selected entry.
    pub(crate) selected: usize,
    /// Whether to draw with ASCII characters only, for terminals without
    /// line-drawing characters.
    pub(crate) ascii: bool,
}

impl Model {
    /// The menu for `snapshot`, with the first default boot environment
    /// selected.
    pub fn new(snapshot: &Snapshot, ascii: bool) -> Model {
        let mut entries: Vec<Entry> = snapshot
            .boot_environments
            .iter()
            .map(|environment| Entry {
                name: environment.name.clone(),
                is_default: environment.is_default,
                encryption: environment.encryption,
            })
            .collect();
        entries.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
        let matches = filter::matches(&entries, "");
        Model {
            entries,
            filter: String::new(),
            matches,
            selected: 0,
            ascii,
        }
    }

    /// The boot environments, defaults first.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The filter text.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// The entries the filter matches, best first.
    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    /// The selected entry, if any entry matches.
    pub fn selected(&self) -> Option<&Entry> {
        let found = self.matches.get(self.selected)?;
        self.entries.get(found.entry)
    }

    /// The index in [`Model::matches`] of the selected entry.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Whether the menu draws with ASCII characters only.
    pub fn is_ascii(&self) -> bool {
        self.ascii
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn defaults_come_first_and_are_selected() {
        let model = Model::new(&Snapshot::example(), false);

        let names: Vec<&str> = model.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["zroot/ROOT/void", "tank/ROOT/arch", "zroot/ROOT/ubuntu"]
        );
        assert_eq!(
            model.selected().map(|e| e.name.as_str()),
            Some("zroot/ROOT/void")
        );
    }

    #[test]
    fn an_empty_machine_has_nothing_selected() {
        let model = Model::new(&Snapshot::default(), false);

        assert_eq!(model.selected(), None);
    }
}
