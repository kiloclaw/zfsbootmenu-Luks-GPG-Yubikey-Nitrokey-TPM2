//! What the machine can do ([`Offer`]): one kind per thing a user starts,
//! which the [`Snapshot`] lists and the menu alone shows (ADR 0016,
//! point 2).

use super::{Action, Snapshot};

/// A kind of action a user starts, which the machine offers or not.
/// Replies to a request (unlocking, skipping, answering) and the dry run
/// are no kind: they are always free.
#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub enum Offer {
    /// Booting an environment.
    Boot,
    /// Booting a snapshot: a new clone of it.
    BootSnapshot,
    /// Making an environment the default of its pool.
    SetDefaultEnvironment,
    /// Setting or inheriting the default kernel of an environment.
    DefaultKernel,
    /// Taking a snapshot.
    CreateSnapshot,
    /// Rolling an environment back to a snapshot.
    Rollback,
    /// Cloning a snapshot, and promoting the clone.
    Clone,
    /// Copying a snapshot into an independent environment.
    Duplicate,
    /// The differences since a snapshot.
    Diff,
    /// Restoring files from a snapshot.
    RestoreFiles,
    /// A chroot of an environment or a snapshot.
    Chroot,
    /// Importing a pool, again or for the first time.
    Import,
    /// Exporting a pool.
    Export,
    /// Opening a LUKS volume.
    OpenVolume,
    /// Sealing a volume's TPM2 key again.
    Reseal,
    /// Giving an environment its boot tries back.
    ResetBootTries,
    /// The recovery shell.
    Shell,
    /// Rebooting into the firmware's setup.
    FirmwareSetup,
    /// Rebooting and powering off.
    Power,
}

impl Offer {
    /// Every kind, in order.
    pub const ALL: [Offer; 19] = [
        Offer::Boot,
        Offer::BootSnapshot,
        Offer::SetDefaultEnvironment,
        Offer::DefaultKernel,
        Offer::CreateSnapshot,
        Offer::Rollback,
        Offer::Clone,
        Offer::Duplicate,
        Offer::Diff,
        Offer::RestoreFiles,
        Offer::Chroot,
        Offer::Import,
        Offer::Export,
        Offer::OpenVolume,
        Offer::Reseal,
        Offer::ResetBootTries,
        Offer::Shell,
        Offer::FirmwareSetup,
        Offer::Power,
    ];
}

impl Action {
    /// The kind of the action, which the machine must offer for the menu
    /// to send it; `None` for a reply to a request, what only reads the
    /// machine ([`Action::is_read`]), the dry run of a change among it, and
    /// the actions of `zluks` alone, which the menu never sends.
    pub fn offer(&self) -> Option<Offer> {
        Some(match self {
            Action::Boot { .. } => Offer::Boot,
            Action::BootSnapshot(..) => Offer::BootSnapshot,
            Action::SetDefaultEnvironment(_) => Offer::SetDefaultEnvironment,
            Action::SetDefaultKernel(..) | Action::InheritDefaultKernel(_) => Offer::DefaultKernel,
            Action::CreateSnapshot(_) => Offer::CreateSnapshot,
            Action::Rollback(_) => Offer::Rollback,
            Action::Clone(..) | Action::CloneAndPromote(..) => Offer::Clone,
            Action::Duplicate(..) => Offer::Duplicate,
            Action::Diff(..) => Offer::Diff,
            Action::RestoreFiles { .. } => Offer::RestoreFiles,
            Action::ResetBootTries(_) => Offer::ResetBootTries,
            Action::Reseal(_) => Offer::Reseal,
            Action::Chroot(_) => Offer::Chroot,
            Action::Import(..) => Offer::Import,
            Action::Export(_) => Offer::Export,
            Action::Open { .. } => Offer::OpenVolume,
            Action::Shell => Offer::Shell,
            Action::FirmwareSetup => Offer::FirmwareSetup,
            Action::Reboot | Action::PowerOff => Offer::Power,
            Action::Unlock { .. }
            | Action::SkipUnlock(_)
            | Action::Answer(_)
            | Action::Plan(_)
            | Action::ScanPools
            | Action::LoadDeviceLog(_)
            | Action::LoadSnapshots(_)
            | Action::LoadHistory
            | Action::UnlockAll
            | Action::OpenWithKeyFile { .. }
            | Action::Close(_) => return None,
        })
    }
}

impl Snapshot {
    /// Whether the machine offers `action`: it has no kind, or the machine
    /// offers its kind.
    pub fn is_offered(&self, action: &Action) -> bool {
        action
            .offer()
            .is_none_or(|kind| self.offers.contains(&kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_is_listed_once_in_order() {
        let mut sorted = Offer::ALL.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, Offer::ALL);
    }

    #[test]
    fn replies_and_the_dry_run_are_free_and_kinds_must_be_offered() {
        let rollback = Action::Rollback("tank/ROOT/be@old".parse().unwrap());
        let mut snapshot = Snapshot::default();

        assert!(snapshot.is_offered(&Action::SkipUnlock("a".to_owned())));
        assert!(snapshot.is_offered(&Action::Plan(Box::new(rollback.clone()))));
        assert!(!snapshot.is_offered(&rollback));
        assert!(!snapshot.is_offered(&Action::Reboot));
        snapshot.offers.insert(Offer::Rollback);
        assert!(snapshot.is_offered(&rollback));
        assert_eq!(Action::PowerOff.offer(), Some(Offer::Power));
        assert_eq!(Action::UnlockAll.offer(), None);
    }

    #[test]
    fn a_snapshot_without_offers_reads_as_offering_nothing() {
        let mut fields = serde_json::to_value(Snapshot::default()).unwrap();
        fields.as_object_mut().unwrap().remove("offers");

        let read: Snapshot = serde_json::from_value(fields).unwrap();
        assert!(read.offers.is_empty());
    }
}
