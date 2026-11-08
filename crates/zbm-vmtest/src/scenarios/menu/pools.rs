//! `menu-pools`: an import of each kind through the menu, read/write and
//! back, a rewind, a forced import and an export, the unlock of the LUKS
//! volume, and last the import that takes the host ID of `tforeign`'s last
//! host, as the step table says; then, in the recovery shell, what they
//! did to the pools.

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
    let mut vm = super::boot_on_fresh(test, "pools", APPEND, &[])?;
    drive(test, &mut vm, &mut Serial { pace: SERIAL })
}

/// The tasks and their checks, from the countdown to the power off.
fn drive(test: &Test, vm: &mut Vm<'_>, serial: &mut Serial) -> Result<()> {
    super::start(test, serial, vm)?;
    steps::run(test, serial, vm, steps::POOLS)?;
    super::shell(test, serial, vm)?;
    checks::pools_left(test, vm)?;
    super::leave_shell(test, serial, vm)?;
    super::power_off(test, serial, vm)
}

#[cfg(test)]
mod tests;
