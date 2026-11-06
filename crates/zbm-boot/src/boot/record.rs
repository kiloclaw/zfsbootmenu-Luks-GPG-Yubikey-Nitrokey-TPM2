//! What the boot sequence saves before the teardown hooks: the logs of the
//! boot and its record, `boot.json` (ADR 0016, point 7; ROADMAP.md #9).

use std::path::Path;

use zbm_bootenv::kernels::Kernel;
use zbm_platform::Context;
use zbm_state::State;
use zbm_types::code::{Coded as _, chain};
use zbm_types::{BootRecord, Untrusted};

use super::sequence::BootSettings;
use crate::load::KexecLoad;
use crate::saved_logs::{self, Destination, Saved};

/// How the kernel of a boot was loaded: what loaded it, and whether it was
/// the copy measured into the TPM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct HowLoaded {
    /// `kexec_file_load`, or `kexec`.
    pub(super) by: &'static str,
    /// Whether the kernel and the initramfs loaded were their measured
    /// copies, which the TPM's events are of.
    pub(super) measured: bool,
}

/// The record of the boot of `selected` with `load`, loaded as `how`, for
/// `settings`. Its time is set when it is saved.
pub(super) fn record(
    settings: &BootSettings,
    selected: &Kernel,
    load: &KexecLoad<'_>,
    how: HowLoaded,
) -> BootRecord {
    let text = |text: &str| Untrusted::new(text.to_owned());
    BootRecord {
        time: 0,
        environment: text(&selected.dataset),
        kernel: text(&selected.kernel),
        initramfs: text(&selected.initramfs),
        command_line: Untrusted::new(load.command_line()),
        devicetree: load.dtb.map(|tree| {
            let inside = Path::new("/").join(tree.strip_prefix(load.mnt).unwrap_or(tree));
            Untrusted::new(inside.to_string_lossy().into_owned())
        }),
        loaded_by: text(how.by),
        measured: how.measured,
        command_line_edited: settings.command_line.is_some(),
    }
}

/// Saves the logs of this boot to `destination`, if given, with `record`,
/// once the kernel is loaded and before the teardown hooks, which may
/// unbind the device the partition is on; a failure is logged and the boot
/// goes on.
pub(crate) fn save_logs(
    context: &Context<'_>,
    state: &State<'_>,
    destination: Option<&Destination>,
    record: Option<&BootRecord>,
) -> Option<Saved> {
    match saved_logs::save(context, state.base(), destination?, record) {
        Ok(saved) => {
            zbm_trace::info!(
                "saved the logs on {device} in {directory}";
                device = %saved.device, directory = %saved.directory
            );
            Some(saved)
        }
        Err(error) => {
            zbm_trace::warn!(
                "unable to save the logs: {error}";
                error = %chain(&error), error_code = %error.code()
            );
            None
        }
    }
}
