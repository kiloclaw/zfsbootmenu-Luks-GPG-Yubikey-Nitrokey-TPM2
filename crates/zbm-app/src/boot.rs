//! Booting and leaving: the use cases of the actions that take the console
//! (ROADMAP.md, R5 step 3). Booting loads the key of the
//! environment's encryption root, finds its kernels, writes the command
//! line chosen for this boot and executes the kernel with `kexec`, as the
//! legacy menu's boot does; the power actions export the pools imported
//! read/write first.

use std::path::Path;
use zbm_types::Untrusted;

use zbm_boot::BootSettings;
use zbm_boot::Recovery;
use zbm_bootenv::BootEnvError;
use zbm_bootenv::hooks::Variables;
use zbm_bootenv::kernels::Kernel;
use zbm_bootenv::keys::{self, BootPath, KeyLoad};
use zbm_platform::console::Screen;
use zbm_platform::interaction::Interaction;
use zbm_platform::{Context, Files, System};
use zbm_state::State;
use zbm_tools::console::ConsoleInteraction;
use zbm_types::{DatasetName, FilesystemName, PoolName};

use crate::model::{self, Event, Outcome};

/// The EFI variable that asks the firmware to open its setup at the next
/// boot, and the one that says whether it can.
const OS_INDICATIONS: &str =
    "/sys/firmware/efi/efivars/OsIndications-8be4df61-93ca-11d2-aa0d-00e098032b8c";
pub(crate) const OS_INDICATIONS_SUPPORTED: &str =
    "/sys/firmware/efi/efivars/OsIndicationsSupported-8be4df61-93ca-11d2-aa0d-00e098032b8c";

/// `EFI_OS_INDICATIONS_BOOT_TO_FW_UI`, a bit of the 64-bit value of both
/// variables.
const BOOT_TO_FIRMWARE: u64 = 0x01;

/// The attributes the specification gives `OsIndications`, which systemd
/// writes when it creates the variable: non-volatile, boot service and
/// runtime access.
const OS_INDICATIONS_ATTRIBUTES: u32 = 0x07;

/// The attributes and the value of `variable`, an EFI variable as
/// `efivarfs` shows it: four bytes of attributes, then the value, both
/// little-endian; `None` when it holds no 64-bit value, which systemd
/// refuses too.
fn u64_variable(variable: &[u8]) -> Option<(u32, u64)> {
    let (attributes, value) = variable.split_first_chunk()?;
    let value = <[u8; 8]>::try_from(value).ok()?;
    Some((u32::from_le_bytes(*attributes), u64::from_le_bytes(value)))
}

/// The bytes of a variable with `attributes` that holds `value`, as
/// [`u64_variable`] reads them.
fn u64_variable_bytes(attributes: u32, value: u64) -> Vec<u8> {
    [attributes.to_le_bytes().as_slice(), &value.to_le_bytes()].concat()
}

/// What the boot use case works on.
pub(crate) struct Boot<'s, 'a> {
    /// The machine.
    pub system: &'s System<'a>,
    /// The state directory.
    pub state: &'s State<'a>,
    /// How to boot: the hooks, the host ID, the saved logs.
    pub settings: &'s BootSettings,
}

/// The variables the hooks of `settings` see now: those of `settings`, with
/// the default boot environment of `state` as it is now, which the menu may
/// have changed since it started.
fn env_now(settings: &BootSettings, state: &State<'_>) -> Variables {
    let env = settings.env.clone();
    match state.bootfs().and_then(|name| name.parse().ok()) {
        Some(bootfs) => env.with_bootfs(&bootfs),
        None => env,
    }
}

/// Loads the key of the encryption root of `dataset` if it is locked, as a
/// boot chosen in the menu loads it: with the key hooks of `settings`,
/// which see the variables of now ([`env_now`]), asking through
/// `interaction`. The menu's changes and its chroot load keys so.
///
/// # Errors
///
/// Why the key stays unloaded.
pub(crate) fn load_menu_key(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    state: &State<'_>,
    settings: &BootSettings,
    dataset: &DatasetName,
) -> Result<(), BootEnvError> {
    let env = env_now(settings, state);
    let how = KeyLoad::new(&settings.hooks, &env, BootPath::Menu);
    keys::load_key(context, interaction, state, &how, dataset)
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

impl<'a> Boot<'_, 'a> {
    /// The ports of the machine, its console left to the front end.
    fn context(&self) -> Context<'a> {
        Context::new(*self.system)
    }

    /// The kernels of `fs`, found by mounting it read-only, newest first as
    /// the menu shows them, with the one that boots marked when
    /// `org.zfsbootmenu:kernel` names it; none when it cannot be mounted.
    /// The kernel list is oldest first, and without the property the last,
    /// the newest, boots: the first here.
    pub fn kernels(&self, environment: &DatasetName) -> Vec<model::Kernel> {
        let fs = FilesystemName::from(environment.clone());
        if zbm_bootenv::find_kernels(&self.context(), self.state, &fs).is_err() {
            return Vec::new();
        }
        let list = self
            .system
            .files
            .read_to_string(&self.state.kernels(environment.as_str()))
            .unwrap_or_default();
        let context = self.context();
        let named = zbm_bootenv::kernel_property(&context, &fs).is_some();
        let chosen = zbm_bootenv::select_kernel(&context, self.state, &fs);
        list.lines()
            .rev()
            .map(Kernel::parse)
            .map(|kernel| model::Kernel {
                version: Untrusted::new(kernel_version(&kernel.kernel)),
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
        recovery: &dyn Recovery,
    ) -> Vec<Event> {
        let fs = environment.as_str();
        let path = if countdown {
            BootPath::Countdown
        } else {
            BootPath::Menu
        };
        let mut settings = self.settings_now();
        let how = KeyLoad::new(&settings.hooks, &settings.env, path);
        let context = self.context();
        let interaction = ConsoleInteraction::new(*self.system);
        if let Err(error) = keys::load_key(&context, &interaction, self.state, &how, environment) {
            return failed(
                format!("unable to load key for {fs}"),
                vec![error.to_string()],
            );
        }
        let filesystem = FilesystemName::from(environment.clone());
        if let Err(error) = zbm_bootenv::find_kernels(&context, self.state, &filesystem) {
            return failed(format!("unable to boot {fs}"), vec![error.to_string()]);
        }
        let Some(selected) = self.selected(&filesystem, kernel) else {
            let version = kernel.unwrap_or_default();
            return failed(
                format!("unable to boot {fs}"),
                vec![format!("it has no kernel {version}")],
            );
        };
        // The typed line is for this boot only: nothing is written down.
        settings.command_line = command_line.map(str::to_owned);
        let ports = (&context, &interaction as &dyn Interaction);
        self.kexec(ports, &settings, environment, &selected, recovery)
    }

    /// Boots `kernel`, a kernel named by its files rather than found in its
    /// boot environment's kernel list, as a boot chosen in the menu boots
    /// one: the key loaded first ([`load_menu_key`]), the boot environment
    /// searched for kernels again, which takes its command line files, then
    /// the settings of now and the built-in steps. A kernel of what is no
    /// boot environment, such as a snapshot, does not boot. Returns only
    /// when the boot did not happen, with the reason as the last event.
    pub fn boot_files(&self, kernel: &Kernel, recovery: &dyn Recovery) -> Vec<Event> {
        let Ok(environment) = kernel.dataset.parse::<DatasetName>() else {
            return failed(
                format!("unable to boot {}", kernel.dataset),
                vec!["it is not a boot environment".to_owned()],
            );
        };
        let context = self.context();
        let interaction = ConsoleInteraction::new(*self.system);
        let state = self.state;
        if let Err(error) =
            load_menu_key(&context, &interaction, state, self.settings, &environment)
        {
            return failed(
                format!("unable to load key for {environment}"),
                vec![error.to_string()],
            );
        }
        // As before a boot from the menu: the command line cache starts
        // afresh from the kernels' files. A kernel named by its files boots
        // even when the search selects none.
        let filesystem = FilesystemName::from(environment.clone());
        let _ = zbm_bootenv::find_kernels(&context, state, &filesystem);
        let settings = self.settings_now();
        let ports = (&context, &interaction as &dyn Interaction);
        self.kexec(ports, &settings, &environment, kernel, recovery)
    }

    /// Boots `selected`, a kernel of `environment` whose key is loaded,
    /// with the ports and the interaction of `ports` and `settings`.
    /// Returns only when the boot did not happen, with the reason as the
    /// last event.
    fn kexec(
        &self,
        (context, interaction): (&Context<'_>, &dyn Interaction),
        settings: &BootSettings,
        environment: &DatasetName,
        selected: &Kernel,
        recovery: &dyn Recovery,
    ) -> Vec<Event> {
        // The plain console boots on a clear screen, as the legacy menu's
        // boot does once the key is loaded.
        self.system.console.screen(Screen::ShowCursor);
        self.system.console.screen(Screen::Clear);
        zbm_boot::kexec_kernel(
            context,
            interaction,
            self.state,
            settings,
            selected,
            recovery,
        );
        failed(
            format!("unable to boot {environment}"),
            vec!["the kernel could not be executed".to_owned()],
        )
    }

    /// The settings of a boot now: the hooks see the default boot
    /// environment as it is now, which the menu may have changed since it
    /// started, and the built-in steps but those `zbm.skip_hooks` left out at
    /// start-up (ADR 0017).
    fn settings_now(&self) -> BootSettings {
        let mut settings = self.settings.clone();
        settings.env = env_now(self.settings, self.state);
        settings.steps = crate::steps::built_in(self.state);
        settings
    }

    /// The file name in `/boot` of the kernel of version `version` of
    /// `environment`, found by mounting it read-only: `vmlinuz-6.12.9_1`;
    /// `None` when it has no such kernel.
    pub fn kernel_file(&self, environment: &DatasetName, version: &str) -> Option<String> {
        let fs = FilesystemName::from(environment.clone());
        zbm_bootenv::find_kernels(&self.context(), self.state, &fs).ok()?;
        let kernel = self.selected(&fs, Some(version))?;
        // As the legacy menu took it from the path: without `/` and `boot/`.
        let path = kernel.kernel.strip_prefix('/').unwrap_or(&kernel.kernel);
        Some(path.strip_prefix("boot/").unwrap_or(path).to_owned())
    }

    /// The kernel of `fs` to boot: the one of version `version`, or the one
    /// its kernel list selects.
    fn selected(&self, fs: &FilesystemName, version: Option<&str>) -> Option<Kernel> {
        let Some(version) = version else {
            return zbm_bootenv::select_kernel(&self.context(), self.state, fs);
        };
        let list = self
            .system
            .files
            .read_to_string(&self.state.kernels(&fs.to_string()))
            .unwrap_or_default();
        list.lines()
            .rev()
            .map(Kernel::parse)
            .find(|kernel| kernel_version(&kernel.kernel) == version)
    }
}

/// Exports those of `pools` that are imported read/write, so that they are
/// clean when the machine resets.
fn export_writable(system: &System<'_>, pools: &[PoolName]) {
    for pool in pools {
        if zbm_pool::writable::is_writable(system.runner, pool) {
            let _ = zbm_boot::export_pool(system.runner, pool);
        }
    }
}

/// Whether the firmware of the machine of `files` can open its setup at the
/// next boot, as `OsIndicationsSupported` says; why not when it cannot.
///
/// # Errors
///
/// What stands in the way: no EFI, which a machine that boots from a BIOS
/// has not, or no support for it, which a variable that holds no 64-bit
/// value gives none of, as systemd reads it.
pub(crate) fn can_open_firmware_setup(files: &dyn Files) -> Result<(), String> {
    let supported = files
        .read(Path::new(OS_INDICATIONS_SUPPORTED))
        .map_err(|_| "the firmware does not say that it can open its setup".to_owned())?;
    match u64_variable(&supported) {
        Some((_, bits)) if bits & BOOT_TO_FIRMWARE != 0 => Ok(()),
        _ => Err("the firmware cannot open its setup at the next boot".to_owned()),
    }
}

/// Asks the firmware to open its setup at the next boot; why not when it
/// cannot. A firmware without `OsIndications`, as some remove it when no
/// bit is set, gets one with [`OS_INDICATIONS_ATTRIBUTES`], as systemd
/// creates it; the attributes and the value go in one write, as `efivarfs`
/// needs.
///
/// # Errors
///
/// What stands in the way: no EFI, no support for it, or a variable that
/// cannot be read, holds no 64-bit value, as systemd refuses it, or cannot
/// be written.
fn request_firmware_setup(system: &System<'_>) -> Result<(), String> {
    let files = system.files;
    can_open_firmware_setup(files)?;
    zbm_boot::mount_efivarfs(&Context::new(*system), "rw");
    let (attributes, bits) = match files.read(Path::new(OS_INDICATIONS)) {
        Ok(read) => u64_variable(&read)
            .ok_or_else(|| "the OsIndications variable holds no 64-bit value".to_owned())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (OS_INDICATIONS_ATTRIBUTES, 0)
        }
        Err(error) => return Err(format!("OsIndications cannot be read: {error}")),
    };
    if bits & BOOT_TO_FIRMWARE == 0 {
        let variable = u64_variable_bytes(attributes, bits | BOOT_TO_FIRMWARE);
        files
            .write(Path::new(OS_INDICATIONS), &variable)
            .map_err(|error| format!("OsIndications cannot be written: {error}"))?;
    }
    Ok(())
}

/// Resets the machine with the kernel's `SysRq` `trigger` (`b` reboots, `o`
/// powers off), after exporting those of `pools` imported read/write;
/// returns only when that failed.
pub(crate) fn reset(system: &System<'_>, pools: &[PoolName], trigger: &str) -> Vec<Event> {
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

/// Reboots into the firmware setup, after exporting those of `pools`
/// imported read/write; returns only when that failed.
pub(crate) fn firmware_setup(system: &System<'_>, pools: &[PoolName]) -> Vec<Event> {
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
