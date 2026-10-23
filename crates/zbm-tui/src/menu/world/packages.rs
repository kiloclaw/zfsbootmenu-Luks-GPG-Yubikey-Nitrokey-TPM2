//! The packages that changed since a snapshot, read from the package
//! database in the snapshot and in the live filesystem
//! (`docs/dev/menu-design/boot-health.md`).

/// What happened to a package since the snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageKind {
    /// Installed since.
    Installed,
    /// Removed since.
    Removed,
    /// A newer version since.
    Upgraded,
    /// An older version since.
    Downgraded,
}

impl PackageKind {
    /// The mark of the list: `+`, `-`, `↑` or `↓` (`^` and `v` in ASCII).
    pub fn mark(self, ascii: bool) -> &'static str {
        match (self, ascii) {
            (PackageKind::Installed, _) => "+",
            (PackageKind::Removed, _) => "-",
            (PackageKind::Upgraded, false) => "↑",
            (PackageKind::Upgraded, true) => "^",
            (PackageKind::Downgraded, false) => "↓",
            (PackageKind::Downgraded, true) => "v",
        }
    }

    /// What happened, in a word.
    pub fn as_str(self) -> &'static str {
        match self {
            PackageKind::Installed => "installed",
            PackageKind::Removed => "removed",
            PackageKind::Upgraded => "upgraded",
            PackageKind::Downgraded => "downgraded",
        }
    }
}

/// One package that changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageChange {
    /// Its name.
    pub name: String,
    /// What happened.
    pub kind: PackageKind,
    /// Its version in the snapshot, unless installed since.
    pub before: Option<String>,
    /// Its version now, unless removed since.
    pub after: Option<String>,
    /// The changed paths of the snapshot's diff that belong to it.
    pub files: Vec<String>,
}

impl PackageChange {
    /// The versions, for people: `24.1.7_1 → 24.2.4_1`.
    pub fn versions(&self, ascii: bool) -> String {
        let arrow = if ascii { "->" } else { "→" };
        match (&self.before, &self.after) {
            (Some(before), Some(after)) => format!("{before} {arrow} {after}"),
            (Some(version), None) | (None, Some(version)) => version.clone(),
            (None, None) => String::new(),
        }
    }
}

/// The packages that changed, and the package manager that says so.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDiff {
    /// The package manager whose database was read: `xbps`, `dpkg`,
    /// `pacman`, `rpm`.
    pub manager: String,
    /// What changed, by name.
    pub changes: Vec<PackageChange>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_marks_read_as_what_happened() {
        let change = |kind, before: Option<&str>, after: Option<&str>| PackageChange {
            name: "mesa".into(),
            kind,
            before: before.map(Into::into),
            after: after.map(Into::into),
            files: Vec::new(),
        };

        let upgrade = change(PackageKind::Upgraded, Some("24.1"), Some("24.2"));
        assert_eq!(upgrade.versions(false), "24.1 → 24.2");
        assert_eq!(upgrade.versions(true), "24.1 -> 24.2");
        assert_eq!(
            change(PackageKind::Removed, Some("2.4"), None).versions(false),
            "2.4"
        );
        assert_eq!(
            change(PackageKind::Installed, None, Some("560")).versions(false),
            "560"
        );
        assert_eq!(
            change(PackageKind::Installed, None, None).versions(false),
            ""
        );
        let marks: Vec<&str> = [
            PackageKind::Installed,
            PackageKind::Removed,
            PackageKind::Upgraded,
            PackageKind::Downgraded,
        ]
        .iter()
        .flat_map(|kind| [kind.mark(false), kind.mark(true)])
        .collect();
        assert_eq!(marks, ["+", "+", "-", "-", "↑", "^", "↓", "v"]);
        assert_eq!(PackageKind::Downgraded.as_str(), "downgraded");
    }
}
