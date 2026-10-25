//! The new menu of the Rust runtime, as a boot meets it.

use std::thread;
use std::time::Duration;

use super::prompt;
use crate::error::Result;
use crate::vm::Vm;

/// Opens the recovery shell from the new menu, once it shows: `!` for the
/// recovery tools, then Enter on the first of them, the shell. The menu
/// ends, and the start-up program opens the shell.
///
/// # Errors
///
/// When the menu does not show.
pub fn shell_from_menu(vm: &mut Vm<'_>) -> Result<()> {
    vm.expect1(prompt::NEW_MENU, 1200)?;
    ask_menu_for_shell(vm)
}

/// Asks the new menu, which shows, for the recovery shell; see
/// [`shell_from_menu`].
///
/// # Errors
///
/// When the keys cannot be sent.
pub fn ask_menu_for_shell(vm: &mut Vm<'_>) -> Result<()> {
    vm.test().step("menu -> ! Enter: the recovery shell");
    thread::sleep(Duration::from_secs(2));
    vm.send("!")?;
    thread::sleep(Duration::from_secs(1));
    vm.send("\r")
}

/// Opens the recovery shell after a ZFS key could not be loaded: the
/// legacy runtime and the Rust one without the menu are in it already;
/// the menu shows the error, which Enter closes, then gives the shell.
///
/// # Errors
///
/// When the keys cannot be sent.
pub fn shell_after_keyload(vm: &mut Vm<'_>) -> Result<()> {
    let screen = vm.console().screen();
    if screen.contains("unable to load key") && screen.contains("BOOT ENVIRONMENTS") {
        vm.test().step("the menu shows the error -> Enter");
        thread::sleep(Duration::from_secs(1));
        vm.send("\r")?;
        ask_menu_for_shell(vm)?;
    }
    vm.shell_ready()
}
