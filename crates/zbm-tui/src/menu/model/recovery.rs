//! The recovery tools, of which the list shows those the machine offers.

use super::ListState;
use super::list::refilter;
use crate::menu::world::{Offer, Snapshot};
use crate::text::{self, Id, id};

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

impl RecoveryTool {
    /// The kind of action the tool starts; `None` for a view.
    pub fn offer(self) -> Option<Offer> {
        match self {
            RecoveryTool::Shell => Some(Offer::Shell),
            RecoveryTool::Chroot => Some(Offer::Chroot),
            RecoveryTool::Log | RecoveryTool::Pools => None,
            RecoveryTool::FirmwareSetup => Some(Offer::FirmwareSetup),
            RecoveryTool::Reboot | RecoveryTool::PowerOff => Some(Offer::Power),
        }
    }
}

/// The indexes in [`RECOVERY`] of the tools the machine of `snapshot`
/// offers: the views, and the actions of an offered kind.
pub(crate) fn recovery_tools(snapshot: &Snapshot) -> Vec<usize> {
    RECOVERY
        .iter()
        .enumerate()
        .filter(|(_, (_, tool))| {
            tool.offer()
                .is_none_or(|kind| snapshot.offers.contains(&kind))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Matches the list of recovery tools against its filter again: the tools
/// `snapshot` offers, as indexes of [`RECOVERY`].
pub(super) fn refresh(list: &mut ListState, snapshot: &Snapshot) {
    let tools = recovery_tools(snapshot);
    let labels: Vec<String> = tools
        .iter()
        .filter_map(|&index| RECOVERY.get(index))
        .map(|(tool, _)| text::get(*tool))
        .collect();
    refilter(list, labels.iter().map(String::as_str));
    for ranked in &mut list.matches {
        ranked.index = tools.get(ranked.index).copied().unwrap_or(ranked.index);
    }
}
