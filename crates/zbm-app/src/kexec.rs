//! Booting a kernel named by its files, as the recovery shell's `zkexec`
//! does (ADR 0018, point 9): the kernel and initramfs of a boot
//! environment boot through the boot of the menu, with its key loaded
//! first, the settings of now and the built-in steps, so that its hooks
//! learn what loaded the kernel, as those of a boot from the menu do.

use zbm_boot::{BootSettings, Recovery};
use zbm_bootenv::kernels::Kernel;
use zbm_platform::System;
use zbm_state::State;

use crate::boot::Boot;
use crate::model::Event;

/// Boots `kernel`, a kernel named by its files in its boot environment, on
/// the console of `system` with the state directory `state`, as a boot
/// chosen in the menu with `settings` boots one; a failure on the way is
/// `recovery`'s, as for that boot. Returns only when the boot did not
/// happen, with the reason as the last event.
pub fn boot_files(
    system: &System<'_>,
    state: &State<'_>,
    settings: &BootSettings,
    kernel: &Kernel,
    recovery: &dyn Recovery,
) -> Vec<Event> {
    let boot = Boot {
        system,
        state,
        settings,
    };
    boot.boot_files(kernel, recovery)
}
