//! Shadow mode: on a machine booting with the legacy runtime, what the Rust
//! code would have decided, compared with what the legacy runtime did
//! (REFACTOR-ROADMAP.md, "Shadow mode on real machines").
//!
//! A `boot-sel.d` hook runs `zbm shadow` after the legacy runtime has chosen
//! a boot environment and before it loads the kernel, while the pools are
//! still imported. The Rust code then decides from the same inputs, through a
//! runner that can only look (`zbm_platform::ReadOnlyRunner`), and each
//! decision becomes a [`Comparison`]:
//!
//! - the host ID in `/etc/hostid`;
//! - the pools imported, and that they are read-only;
//! - the mapping name of each LUKS volume the legacy runtime opened;
//! - the default boot environment;
//! - the kernel and initramfs of the selected boot environment.
//!
//! Each phase of the refactoring adds the decisions it moves. A decision
//! whose inputs are missing is left out, not counted as a difference.

mod decisions;
pub mod efivar;

use std::fmt::Write as _;

use zbm_platform::System;
use zbm_state::State;

pub use decisions::{boot_environment, host_id, kernel, luks_mappings, pools};

/// What the legacy runtime passes to `boot-sel.d` hooks.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Selected {
    /// `ZBM_SELECTED_BE`: the boot environment chosen.
    pub be: String,
    /// `ZBM_SELECTED_KERNEL`: its kernel, relative to its root.
    pub kernel: String,
    /// `ZBM_SELECTED_INITRAMFS`: its initramfs, relative to its root.
    pub initramfs: String,
    /// `ZBM_SELECTED_MOUNTPOINT`: where it is mounted.
    pub mountpoint: String,
}

impl Selected {
    /// The selection from the environment of a `boot-sel.d` hook.
    pub fn from_environment(environment: &dyn zbm_platform::Environment) -> Selected {
        let get = |name: &str| environment.non_empty(name).unwrap_or_default();
        Selected {
            be: get("ZBM_SELECTED_BE"),
            kernel: get("ZBM_SELECTED_KERNEL"),
            initramfs: get("ZBM_SELECTED_INITRAMFS"),
            mountpoint: get("ZBM_SELECTED_MOUNTPOINT"),
        }
    }
}

/// One decision, as each runtime made it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Comparison {
    /// What was decided, such as `host ID`.
    pub decision: &'static str,
    /// What the legacy runtime did.
    pub legacy: String,
    /// What the Rust code would have done.
    pub rust: String,
}

impl Comparison {
    /// Whether the two runtimes decided differently.
    pub fn is_difference(&self) -> bool {
        self.legacy != self.rust
    }
}

/// The comparisons of one boot.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Report {
    /// Each decision compared.
    pub comparisons: Vec<Comparison>,
}

impl Report {
    /// How many decisions differ.
    pub fn differences(&self) -> usize {
        self.comparisons
            .iter()
            .filter(|comparison| comparison.is_difference())
            .count()
    }

    /// The line the console, the kernel log and the VM tests read: the
    /// number of differences and the decisions compared, such as
    /// `ZBM-SHADOW: 0 differences in 2 decisions (pools, kernel)`.
    pub fn summary(&self) -> String {
        let decisions: Vec<&str> = self.comparisons.iter().map(|c| c.decision).collect();
        let mut line = format!(
            "ZBM-SHADOW: {} differences in {} decisions",
            self.differences(),
            decisions.len()
        );
        if !decisions.is_empty() {
            let _ = write!(line, " ({})", decisions.join(", "));
        }
        line
    }

    /// One line per difference, then the summary.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for comparison in self.comparisons.iter().filter(|c| c.is_difference()) {
            let _ = writeln!(
                text,
                "ZBM-SHADOW: {}: legacy {}, rust {}",
                comparison.decision, comparison.legacy, comparison.rust
            );
        }
        let _ = writeln!(text, "{}", self.summary());
        text
    }
}

/// Every decision that can be compared on `system`, whose runner must only
/// read, with the state directory `state` and the legacy runtime's
/// `selected` boot environment. Each comparison is also a `tracing` event,
/// which the journal records.
pub fn compare(system: &System<'_>, state: &State<'_>, selected: &Selected) -> Report {
    let comparisons: Vec<Comparison> = [
        host_id(system.files, state),
        pools(system.runner, system.files),
        luks_mappings(system, state),
        boot_environment(system.runner, system.files, state),
        kernel(system, selected),
    ]
    .into_iter()
    .flatten()
    .collect();
    for comparison in &comparisons {
        let (decision, legacy, rust) = (comparison.decision, &comparison.legacy, &comparison.rust);
        if comparison.is_difference() {
            tracing::warn!(target: "zbm::shadow", decision, legacy, rust, "difference");
        } else {
            tracing::info!(target: "zbm::shadow", decision, legacy, rust, "same");
        }
    }
    Report { comparisons }
}

#[cfg(test)]
mod tests;
