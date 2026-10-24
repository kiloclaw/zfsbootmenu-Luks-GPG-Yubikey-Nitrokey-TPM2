//! Booting and leaving: the use cases of the actions that take the console
//! (REFACTOR-ROADMAP.md, R5 step 3). Booting loads the key of the
//! environment's encryption root, finds its kernels, writes the command
//! line chosen for this boot and executes the kernel with `kexec`, as the
//! legacy menu's boot does; the power actions export the pools imported
//! read/write first.

use std::path::Path;

use zbm_boot::boot::{self as kexec, BootSettings};
use zbm_boot::kernels::Kernel;
use zbm_boot::keys::{self, BootPath, Clear, KeyLoad};
use zbm_boot::recovery::Recovery;
use zbm_config::kcl;
use zbm_platform::System;
use zbm_state::State;
use zbm_types::DatasetName;

use crate::model::{self, Event, Outcome};

/// The EFI variable that asks the firmware to open its setup at the next
/// boot, and the one that says whether it can.
const OS_INDICATIONS: &str =
    "/sys/firmware/efi/efivars/OsIndications-8be4df61-93ca-11d2-aa0d-00e098032b8c";
const OS_INDICATIONS_SUPPORTED: &str =
    "/sys/firmware/efi/efivars/OsIndicationsSupported-8be4df61-93ca-11d2-aa0d-00e098032b8c";

/// `EFI_OS_INDICATIONS_BOOT_TO_FW_UI`, in the first byte of the variable's
/// value, which follows its four bytes of attributes.
const BOOT_TO_FIRMWARE: u8 = 0x01;

/// What the boot use case works on.
pub(crate) struct Boot<'s, 'a> {
    /// The machine.
    pub system: &'s System<'a>,
    /// The state directory.
    pub state: &'s State<'a>,
    /// How to boot: the hooks, the host ID, the saved logs.
    pub settings: &'s BootSettings,
}

/// A failed outcome, as the last event.
fn failed(what: String, why: Vec<String>) -> Vec<Event> {
    vec![Event::Finished(Outcome::Failed {
        what,
        why,
        next: Vec::new(),
    })]
}

/// The version the menu shows for the kernel at `path`: its file name after
/// the first `-` (`vmlinuz-6.12.9_1` is `6.12.9_1`), or the whole file name.
pub fn kernel_version(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.split_once('-')
        .map_or(name, |(_, version)| version)
        .to_owned()
}

impl Boot<'_, '_> {
    /// The kernels of `fs`, found by mounting it read-only, with the one that
    /// boots marked when `org.zfsbootmenu:kernel` names it; none when it
    /// cannot be mounted.
    pub fn kernels(&self, fs: &str) -> Vec<model::Kernel> {
        if kexec::find_kernels(self.system, self.state, fs).is_err() {
            return Vec::new();
        }
        let list = self
            .system
            .files
            .read_to_string(&self.state.kernels(fs))
            .unwrap_or_default();
        let named = kexec::kernel_property(self.system, fs).is_some();
        let chosen = kexec::select_kernel(self.system, self.state, fs);
        list.lines()
            .map(Kernel::parse)
            .map(|kernel| model::Kernel {
                version: kernel_version(&kernel.kernel),
                is_default: named && chosen.as_ref() == Some(&kernel),
            })
            .collect()
    }

    /// Boots `environment` with the kernel of version `kernel`, or its own,
    /// and `command_line` for this boot, or its own; the countdown's boot
    /// (`countdown`) tries no key source. Returns only when the boot did
    /// not happen, with the reason as the last event.
    pub fn boot(
        &self,
        environment: &DatasetName,
        kernel: Option<&str>,
        command_line: Option<&str>,
        countdown: bool,
        recovery: &mut dyn Recovery,
    ) -> Vec<Event> {
        let fs = environment.as_str();
        let how = KeyLoad {
            hooks: &self.settings.hooks,
            path: if countdown {
                BootPath::Countdown
            } else {
                BootPath::Menu
            },
            clear: Clear::Yes,
        };
        if let Err(error) = keys::load_key(self.system, self.state, &how, fs) {
            return failed(
                format!("unable to load key for {fs}"),
                vec![error.to_string()],
            );
        }
        if let Err(error) = kexec::find_kernels(self.system, self.state, fs) {
            return failed(format!("unable to boot {fs}"), vec![error.to_string()]);
        }
        let Some(selected) = self.selected(fs, kernel) else {
            let version = kernel.unwrap_or_default();
            return failed(
                format!("unable to boot {fs}"),
                vec![format!("it has no kernel {version}")],
            );
        };
        if let Some(line) = command_line
            && let Err(error) = self.state.set_cmdline_override(&kcl::tokenize(line))
        {
            return failed(format!("unable to boot {fs}"), vec![error.to_string()]);
        }
        kexec::kexec_kernel(self.system, self.state, self.settings, &selected, recovery);
        failed(
            format!("unable to boot {fs}"),
            vec!["the kernel could not be executed".to_owned()],
        )
    }

    /// The kernel of `fs` to boot: the one of version `version`, or the one
    /// its kernel list selects.
    fn selected(&self, fs: &str, version: Option<&str>) -> Option<Kernel> {
        let Some(version) = version else {
            return kexec::select_kernel(self.system, self.state, fs);
        };
        let list = self
            .system
            .files
            .read_to_string(&self.state.kernels(fs))
            .unwrap_or_default();
        list.lines()
            .map(Kernel::parse)
            .find(|kernel| kernel_version(&kernel.kernel) == version)
    }
}

/// Exports the pools imported read/write, so that they are clean when the
/// machine resets.
fn export_writable(system: &System<'_>, pools: &[model::Pool]) {
    for pool in pools.iter().filter(|pool| pool.imported) {
        let name = pool.name.as_str();
        if kexec::is_writable(system.runner, name) {
            let _ = kexec::export_pool(system.runner, name);
        }
    }
}

/// Asks the firmware to open its setup at the next boot; why not when it
/// cannot.
///
/// # Errors
///
/// What stands in the way: no EFI, no support for it, or a variable that
/// cannot be written.
fn request_firmware_setup(system: &System<'_>) -> Result<(), String> {
    let files = system.files;
    let supported = files
        .read(Path::new(OS_INDICATIONS_SUPPORTED))
        .map_err(|_| "the firmware does not say that it can open its setup".to_owned())?;
    if supported
        .get(4)
        .is_none_or(|bits| bits & BOOT_TO_FIRMWARE == 0)
    {
        return Err("the firmware cannot open its setup at the next boot".to_owned());
    }
    kexec::mount_efivarfs(system, "rw");
    let mut current = files
        .read(Path::new(OS_INDICATIONS))
        .map_err(|_| "the firmware has no OsIndications variable".to_owned())?;
    let Some(bits) = current.get_mut(4) else {
        return Err("the OsIndications variable is too short".to_owned());
    };
    if *bits & BOOT_TO_FIRMWARE == 0 {
        *bits |= BOOT_TO_FIRMWARE;
        files
            .write(Path::new(OS_INDICATIONS), &current)
            .map_err(|error| format!("OsIndications cannot be written: {error}"))?;
    }
    Ok(())
}

/// Resets the machine with the kernel's `SysRq` `trigger` (`b` reboots, `o`
/// powers off), after exporting the pools imported read/write; returns only
/// when that failed.
pub(crate) fn reset(system: &System<'_>, pools: &[model::Pool], trigger: &str) -> Vec<Event> {
    export_writable(system, pools);
    let written = system
        .files
        .write(Path::new("/proc/sysrq-trigger"), trigger.as_bytes());
    failed(
        "unable to reset the machine".to_owned(),
        vec![match written {
            Ok(()) => "the machine did not reset".to_owned(),
            Err(error) => format!("/proc/sysrq-trigger: {error}"),
        }],
    )
}

/// Reboots into the firmware setup; returns only when that failed.
pub(crate) fn firmware_setup(system: &System<'_>, pools: &[model::Pool]) -> Vec<Event> {
    match request_firmware_setup(system) {
        Ok(()) => reset(system, pools, "b"),
        Err(why) => failed(
            "unable to reboot into the firmware setup".to_owned(),
            vec![why],
        ),
    }
}

#[cfg(test)]
mod tests;
