//! The differences between a snapshot and the live filesystem, as `zfs
//! diff` reports them, and what can be restored from the snapshot.

/// What happened to a path since the snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeKind {
    /// Removed since: the snapshot has it.
    Removed,
    /// Created since: the snapshot does not have it.
    Created,
    /// Modified since.
    Modified,
    /// Renamed since.
    Renamed,
}

impl ChangeKind {
    /// The mark `zfs diff` uses.
    pub fn mark(self) -> &'static str {
        match self {
            ChangeKind::Removed => "-",
            ChangeKind::Created => "+",
            ChangeKind::Modified => "M",
            ChangeKind::Renamed => "R",
        }
    }
}

/// One changed path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Change {
    /// What happened.
    pub kind: ChangeKind,
    /// The path, in the snapshot.
    pub path: String,
    /// The new path of a renamed one.
    pub renamed_to: Option<String>,
    /// The lines of a text diff, `-` and `+` first; empty for binary files.
    pub preview: Vec<String>,
}

impl Change {
    /// Whether the snapshot has the path, so it can be restored from it: not
    /// for paths created since.
    pub fn is_restorable(&self) -> bool {
        self.kind != ChangeKind::Created
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_paths_the_snapshot_has_can_be_restored() {
        let change = |kind| Change {
            kind,
            path: "/etc/fstab".into(),
            renamed_to: None,
            preview: Vec::new(),
        };

        assert!(change(ChangeKind::Modified).is_restorable());
        assert!(change(ChangeKind::Removed).is_restorable());
        assert!(change(ChangeKind::Renamed).is_restorable());
        assert!(!change(ChangeKind::Created).is_restorable());
        assert_eq!(ChangeKind::Renamed.mark(), "R");
    }
}
