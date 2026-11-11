//! The kinds of action [`App`] offers on the machine ([`Offer`]): those it
//! performs there; the others end as [`unavailable`](super::unavailable).

use std::collections::BTreeSet;

use super::App;
use crate::boot;
use crate::model::Offer;

impl App<'_> {
    /// The kinds of action the application performs on the machine of
    /// `files`: the boot, a snapshot's too, the changes of package D
    /// ([`Action::is_change`](crate::model::Action::is_change)), the
    /// default kernel and the command line, restoring files, the import
    /// and the export among them, the recovery shell, the chroot, the power
    /// actions, the firmware setup where the firmware says it can open it,
    /// which it does not on a machine that boots from a BIOS, and opening a
    /// volume with a LUKS session in an image with `cryptsetup`. The others
    /// have no use case yet, and end as [`unavailable`](super::unavailable).
    pub(super) fn offers(&self, files: &dyn zbm_platform::Files) -> BTreeSet<Offer> {
        let firmware = boot::can_open_firmware_setup(files)
            .is_ok()
            .then_some(Offer::FirmwareSetup);
        let open = self
            .luks
            .filter(|luks| luks.context.command_exists("cryptsetup"))
            .map(|_| Offer::OpenVolume);
        [
            Offer::Boot,
            Offer::BootSnapshot,
            Offer::SetDefaultEnvironment,
            Offer::DefaultKernel,
            Offer::CommandLine,
            Offer::CreateSnapshot,
            Offer::Rollback,
            Offer::Destroy,
            Offer::Clone,
            Offer::Duplicate,
            Offer::Diff,
            Offer::RestoreFiles,
            Offer::Import,
            Offer::Export,
            Offer::Shell,
            Offer::Chroot,
            Offer::Power,
        ]
        .into_iter()
        .chain(firmware)
        .chain(open)
        .collect()
    }
}
