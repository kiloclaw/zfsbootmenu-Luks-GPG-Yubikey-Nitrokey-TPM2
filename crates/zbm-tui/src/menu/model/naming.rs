//! The name of something new: a snapshot, or the boot environment a
//! snapshot is cloned or copied into. The dialog starts with the name the
//! legacy menu offers, a time stamp for a snapshot and `<environment>_NEW`
//! for an environment, and takes only the characters the legacy menu
//! allows.

use zbm_types::{DatasetName, SnapshotName};

use crate::menu::world::Action;
pub use crate::menu::world::timestamp;

/// What a name is for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Make {
    /// A snapshot of the environment.
    Snapshot(DatasetName),
    /// A clone of the snapshot.
    Clone(SnapshotName),
    /// A clone of the snapshot, promoted.
    Promote(SnapshotName),
    /// An independent copy of the snapshot.
    Duplicate(SnapshotName),
}

/// A name being typed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Naming {
    /// The title.
    pub title: String,
    /// What will happen.
    pub lines: Vec<String>,
    /// What the name goes after: `zroot/ROOT/` or `zroot/ROOT/void@`.
    pub prefix: String,
    /// The name so far.
    pub typed: String,
    /// What it is for.
    pub make: Make,
}

/// Whether `c` may be part of a name: the characters the legacy menu
/// keeps, `[a-zA-Z0-9-_.:]`.
pub fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')
}

impl Naming {
    /// The dialog for `make`, with the legacy menu's name in it; `now` is
    /// the time, in seconds since 1970, a snapshot's name comes from.
    pub fn new(make: Make, now: u64, title: String, lines: Vec<String>) -> Naming {
        let (prefix, typed) = match &make {
            Make::Snapshot(environment) => (format!("{environment}@"), timestamp(now)),
            Make::Clone(snapshot) | Make::Promote(snapshot) | Make::Duplicate(snapshot) => {
                let environment = snapshot.dataset().as_str();
                let (parent, last) = environment
                    .rsplit_once('/')
                    .unwrap_or((environment, environment));
                (format!("{parent}/"), format!("{last}_NEW"))
            }
        };
        Naming {
            title,
            lines,
            prefix,
            typed,
            make,
        }
    }

    /// The full name typed: the prefix and the name.
    pub fn full_name(&self) -> String {
        format!("{}{}", self.prefix, self.typed)
    }

    /// The action that makes it, unless the name is empty or invalid.
    pub fn action(&self) -> Option<Action> {
        if self.typed.is_empty() || !self.typed.chars().all(is_name_char) {
            return None;
        }
        let full = self.full_name();
        Some(match &self.make {
            Make::Snapshot(_) => Action::CreateSnapshot(full.parse().ok()?),
            Make::Clone(snapshot) => Action::Clone(snapshot.clone(), full.parse().ok()?),
            Make::Promote(snapshot) => {
                Action::CloneAndPromote(snapshot.clone(), full.parse().ok()?)
            }
            Make::Duplicate(snapshot) => Action::Duplicate(snapshot.clone(), full.parse().ok()?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_start_as_the_legacy_menu_offers_them() {
        let void: SnapshotName = "zroot/ROOT/void@first".parse().unwrap();
        let clone = Naming::new(Make::Clone(void.clone()), 0, String::new(), Vec::new());
        assert_eq!(clone.full_name(), "zroot/ROOT/void_NEW");
        assert_eq!(
            clone.action(),
            Some(Action::Clone(void, "zroot/ROOT/void_NEW".parse().unwrap()))
        );

        let root: SnapshotName = "zroot@x".parse().unwrap();
        let copy = Naming::new(Make::Duplicate(root), 0, String::new(), Vec::new());
        assert_eq!(copy.full_name(), "zroot/zroot_NEW");

        let environment: DatasetName = "zroot/ROOT/void".parse().unwrap();
        let mut snapshot = Naming::new(Make::Snapshot(environment), 0, String::new(), Vec::new());
        assert_eq!(snapshot.full_name(), "zroot/ROOT/void@1970-01-01-000000");
        snapshot.typed.clear();
        assert_eq!(snapshot.action(), None);
        snapshot.typed = "a b".to_owned();
        assert_eq!(snapshot.action(), None);
        assert!(is_name_char(':') && !is_name_char('/') && !is_name_char('@'));
    }
}
