//! `runtime-rust-recovery`: the recovery tools of the Rust runtime as
//! `zbm`'s applets (ADR 0018). One boot of the image of `runtime-rust`, with
//! `tplain` (d0), on the group's machine, which boots from a BIOS:
//!
//! - the menu's Recovery Tools list no Firmware Setup, which a BIOS cannot
//!   open, but Reboot and Power Off;
//! - in the recovery shell, `firmware-setup` says why and exits 1;
//!   `reboot --help` and `shutdown --help` print the usages of `zbm`'s
//!   `reboot` and `poweroff`, through the image's links. Neither is run,
//!   since either would end the machine.

use std::thread;
use std::time::Duration;

use crate::boot::prompt;
use crate::error::Result;
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// The title of the menu's Recovery Tools.
const TOOLS: &str = "screen:RECOVERY TOOLS";

/// What `firmware-setup` says on a machine that boots from a BIOS.
const NO_FIRMWARE_SETUP: [&str; 2] = [
    "firmware-setup: unable to reboot into the firmware setup",
    "the firmware does not say that it can open its setup",
];

/// How long a step may take, in seconds.
const PATIENCE: u64 = 1200;

/// Boots, looks at the Recovery Tools, then runs the shell's tools.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let image = super::image(settings, "auto", ZbmRuntime::Rust, None)?;
    let machine =
        Machine::boot(&image, super::ESCAPE_APPEND).with_disks(vec![super::disk(settings, 0)?]);
    let mut vm = Vm::start(test, "recovery", &machine)?;
    super::escape_countdown(&mut vm)?;
    tools_to_shell(test, &mut vm, Duration::from_secs(2))?;
    shell_tools(test, &mut vm)?;
    vm.kill();
    Ok(())
}

/// Opens the menu's Recovery Tools, checks what they list on a machine
/// that boots from a BIOS, then opens the recovery shell, the first of
/// them; `pause` is how long to leave the menu between two keys.
fn tools_to_shell(test: &Test, vm: &mut Vm<'_>, pause: Duration) -> Result<()> {
    vm.expect1(prompt::NEW_MENU, PATIENCE)?;
    test.step("menu -> !: the Recovery Tools");
    thread::sleep(pause);
    vm.send("!")?;
    let shown = vm.expect1(TOOLS, 60).is_ok();
    let screen = vm.console().screen();
    test.check(
        shown && screen.contains("Reboot") && screen.contains("Power Off"),
        "the menu's Recovery Tools list Reboot and Power Off",
    );
    test.check(
        shown && !screen.contains("Firmware Setup"),
        "on a BIOS, the menu's Recovery Tools list no Firmware Setup",
    );
    test.step("Enter: the Recovery Shell");
    thread::sleep(pause);
    vm.send("\r")?;
    vm.shell_ready()
}

/// Runs `firmware-setup`, which must say why it cannot, and asks `reboot`
/// and `shutdown` for their usages, which `zbm` prints.
fn shell_tools(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (code, said) = vm.sh_show("firmware-setup")?;
    test.check(
        code == 1,
        &format!("on a BIOS, firmware-setup exits 1 ({code})"),
    );
    test.check(
        NO_FIRMWARE_SETUP.iter().all(|line| said.contains(line)),
        "firmware-setup says why the machine cannot reboot into its setup",
    );
    for (name, usage) in [("reboot", "Usage: reboot"), ("shutdown", "Usage: poweroff")] {
        let (code, said) = vm.sh_show(&format!("{name} --help"))?;
        test.check(
            code == 0 && said.contains(usage),
            &format!("{name} is zbm's: --help prints {usage:?} ({code})"),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
