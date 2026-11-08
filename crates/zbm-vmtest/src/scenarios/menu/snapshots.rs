//! `menu-snapshots`: each change of a snapshot through the menu, a diff
//! and a restore, destroy, and the changes ZFS would refuse, as the step
//! tables say; then, in the recovery shell, what they did to `tmenu`.
//!
//! The shell opens three times. Before and after the destroy of `one` and
//! `three`, which nothing else may change between, so that the space the
//! snapshots hold falls by what `zfs destroy -nvp` said it frees; and last,
//! after the clone and promote, which takes the older snapshots from the
//! boot environment. Each time the menu starts again on its Boot tab.

use crate::error::Result;
use crate::record::Test;
use crate::vm::Vm;

use super::super::drive::Serial;
use super::{APPEND, SERIAL, checks, steps};

/// Boots on fresh disks and drives the menu.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let mut vm = super::boot_on_fresh(test, "snapshots", APPEND, &[])?;
    drive(test, &mut vm, &mut Serial { pace: SERIAL })
}

/// The tasks and their checks, from the countdown to the power off.
fn drive(test: &Test, vm: &mut Vm<'_>, serial: &mut Serial) -> Result<()> {
    super::start(test, serial, vm)?;
    steps::run(test, serial, vm, steps::SNAPSHOTS_CHANGES)?;
    super::shell(test, serial, vm)?;
    let before = checks::space_before(test, vm)?;
    super::leave_shell(test, serial, vm)?;
    steps::run(test, serial, vm, steps::SNAPSHOTS_DESTROY)?;
    super::shell(test, serial, vm)?;
    checks::space_after(test, vm, before)?;
    super::leave_shell(test, serial, vm)?;
    steps::run(test, serial, vm, steps::SNAPSHOTS_REFUSED)?;
    super::shell(test, serial, vm)?;
    checks::snapshots_left(test, vm)?;
    checks::files_restored(test, vm)?;
    super::leave_shell(test, serial, vm)?;
    super::power_off(test, serial, vm)
}

#[cfg(test)]
mod tests;
