//! Boot health: whether an environment will boot (its checks), boot
//! counting and the fallback after failed boots, and the one-time boot
//! (`docs/dev/menu-design/boot-health.md`).

use zbm_types::{DatasetName, SnapshotName};

/// How a check came out.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Verdict {
    /// Nothing in the way.
    Pass,
    /// It may boot, with a problem to know about.
    Warning,
    /// It will not boot as it is.
    Failure,
}

/// One check of whether an environment will boot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Check {
    /// What is checked, for people: `ZFS module`.
    pub name: String,
    /// How it came out.
    pub verdict: Verdict,
    /// What was found: `zfs 2.4.4 for 6.12.9_1`.
    pub detail: String,
    /// What to do about it, when it did not pass.
    pub fix: Option<String>,
}

/// The worst verdict of `checks`, and how many have it; `None` when every
/// check passed or none ran.
pub fn worst(checks: &[Check]) -> Option<(Verdict, usize)> {
    let worst = checks.iter().map(|check| check.verdict).max()?;
    if worst == Verdict::Pass {
        return None;
    }
    let count = checks.iter().filter(|check| check.verdict == worst).count();
    Some((worst, count))
}

/// Boot counting of an environment (`zbm.boot_tries`): each boot uses a
/// try, and the booted system gives them back once it is up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tries {
    /// Tries left.
    pub left: u8,
    /// Tries after a good boot.
    pub total: u8,
}

/// What boots instead of an environment that used up its tries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target {
    /// Another environment: the last one the booted system marked good.
    Environment(DatasetName),
    /// A temporary clone of a snapshot, when no environment was marked good.
    Snapshot(SnapshotName),
}

/// The default environment failed to start: what boots instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fallback {
    /// The environment that used up its tries.
    pub failed: DatasetName,
    /// How many times it failed.
    pub attempts: u8,
    /// What boots instead.
    pub target: Target,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn check(verdict: Verdict) -> Check {
        Check {
            name: "x".into(),
            verdict,
            detail: String::new(),
            fix: None,
        }
    }

    #[test]
    fn the_worst_verdict_counts_its_checks() {
        let mixed = [
            check(Verdict::Pass),
            check(Verdict::Warning),
            check(Verdict::Failure),
            check(Verdict::Warning),
        ];
        let warnings = [check(Verdict::Pass), check(Verdict::Warning)];

        assert_eq!(worst(&mixed), Some((Verdict::Failure, 1)));
        assert_eq!(worst(&warnings), Some((Verdict::Warning, 1)));
        assert_eq!(worst(&[check(Verdict::Pass)]), None);
        assert_eq!(worst(&[]), None);
    }
}
