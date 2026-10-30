//! The boot sequence: from a selected kernel to running it with `kexec`,
//! through the hooks, the command line, the device tree and the teardown of
//! mounts and pools.

use std::path::{Path, PathBuf};
use zbm_platform::console::Screen;
use zbm_types::code::{Coded as _, chain};

use zbm_config::spec::properties::{DEVICETREE, ROOTPREFIX};
use zbm_platform::System;
use zbm_state::State;
use zbm_tools::{kexec, programs};
use zbm_trace::Level;
use zbm_types::{
    DatasetName, FilesystemName, HookStage, ImportPolicy, KernelCommandLine, Printable,
};
use zbm_zfs::ZfsText;

use super::measured;
use super::mounts::{Noise, mount_zfs, recursive_umount, umount};
use super::zfs::{export_writable_pools, substituted, zfs_value};
use crate::commandline::{self, KexecLoad};
use crate::graphics;
use crate::hooks;
use crate::kernels::Kernel;
use crate::keys::{self, BootPath, Clear, KeyLoad};
use crate::prompt::{TimedPrompt, colorize};
use crate::recovery::Recovery;
use crate::saved_logs::{self, Destination, Saved};

/// What booting needs besides the kernel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootSettings {
    /// `zbm_set_hostid`: replace the host ID on the command line.
    pub set_hostid: bool,
    /// Where the hook directories are.
    pub hooks: PathBuf,
    /// Variables the start-up program exported after it started (`BOOTFS`),
    /// which hooks get on top of its environment.
    pub env: Vec<(String, String)>,
    /// `zbm.save_logs`: where the logs of this boot are saved before the
    /// kernel starts, if anywhere (ROADMAP.md #9).
    pub save_logs: Option<Destination>,
    /// A command line typed in the menu for this boot only, in place of the
    /// boot environment's `org.zfsbootmenu:commandline`.
    pub command_line: Option<String>,
    /// `zbm.import_policy`: a read/write import forces (`-f`) with `force`.
    pub import_policy: ImportPolicy,
}

impl Default for BootSettings {
    fn default() -> BootSettings {
        BootSettings {
            set_hostid: true,
            hooks: PathBuf::from(hooks::HOOKS),
            env: Vec::new(),
            save_logs: None,
            command_line: None,
            import_policy: ImportPolicy::default(),
        }
    }
}

/// The root prefix of `fs` mounted at `mnt`, with a debug message naming
/// where it came from.
fn find_root_prefix(system: &System<'_>, fs: &FilesystemName, mnt: &Path) -> String {
    zbm_trace::debug!("zfsbe_fs set to {fs}"; %fs);
    zbm_trace::debug!("zfsbe_mnt set to {mnt}"; mnt = %mnt.display());
    let property = zfs_value(system.runner, ROOTPREFIX.name, fs);
    commandline::root_prefix(system.files, property.as_deref(), mnt)
}

/// Loads the key of `fs` if it is locked, with key sources and after
/// clearing the screen. The countdown has loaded it already, so this loads
/// it for a boot chosen in the menu; a failure shows when mounting.
fn load_key(system: &System<'_>, state: &State<'_>, settings: &BootSettings, fs: &DatasetName) {
    let how = KeyLoad {
        hooks: &settings.hooks,
        path: BootPath::Menu,
        clear: Clear::Yes,
    };
    let _ = keys::load_key(system, state, &how, fs);
}

/// Loads the key of `fs` and mounts it, clearing the screen in between; the
/// filesystem and its mount point, or `None` when it cannot be mounted. A
/// kernel list line whose dataset is no name fails to mount.
fn load_and_mount(
    system: &System<'_>,
    state: &State<'_>,
    settings: &BootSettings,
    fs: &str,
) -> Option<(FilesystemName, PathBuf)> {
    let filesystem = fs.parse::<FilesystemName>().ok();
    if let Some(filesystem) = &filesystem {
        load_key(system, state, settings, filesystem.dataset());
    }
    system.console.screen(Screen::ShowCursor);
    system.console.screen(Screen::Clear);
    let filesystem = filesystem?;
    // Mounted until the teardown, or the recursive unmount of a failure.
    let mnt = mount_zfs(system, state, &filesystem, Noise::Shown)?.leave();
    Some((filesystem, mnt))
}

/// Boots `selected`: mounts it, runs the `boot-sel.d` hooks, loads the kernel
/// with its command line, unmounts it, exports the pools imported read-write,
/// runs the `teardown.d` hooks, has the console on the display hand the
/// display back, and executes the kernel. Returns only if that fails, after
/// telling the user.
pub fn kexec_kernel(
    system: &System<'_>,
    state: &State<'_>,
    settings: &BootSettings,
    selected: &Kernel,
    recovery: &dyn Recovery,
) {
    if !names_a_kernel(selected) {
        return;
    }
    let Kernel {
        dataset: fs,
        kernel,
        initramfs,
    } = selected;
    let Some((filesystem, mnt)) = load_and_mount(system, state, settings, fs) else {
        recovery.emergency_shell(&format!("unable to mount {}", colorize("cyan", fs)));
        return;
    };
    let mnt_text = mnt.to_string_lossy().into_owned();
    let hook_env = hook_env(settings, selected);
    run_boot_sel_hooks(system, settings, &hook_env, &mnt_text);

    let cli_args = command_line(system, state, settings, fs, kernel);
    let root_prefix = find_root_prefix(system, &filesystem, &mnt);
    let Some(root) = commandline::root_argument(&root_prefix, fs) else {
        zbm_trace::error!(
            root_prefix,
            fs,
            "the root argument is not one kernel argument"
        );
        let _ = recursive_umount(system, &mnt);
        recovery.emergency_shell(&format!("unable to boot {}", colorize("cyan", fs)));
        return;
    };
    let dtb = find_devicetree(system, &filesystem, kernel, &mnt);
    let load = KexecLoad {
        mnt: &mnt_text,
        kernel,
        initramfs,
        dtb: dtb.as_deref(),
        root: &root,
        args: &cli_args,
        copies: None,
    };
    let Some(output) = load_measured(system, state, &load, &mnt) else {
        return;
    };
    log_loaded(&load, &output);

    // The logs are saved before the teardown hooks, which may take away
    // the partition they go to (`90-xhci-unbind` and an ESP on USB).
    let saved = save_logs(system, state, settings.save_logs.as_ref());
    tear_down(system, settings, &hook_env, &mnt);
    hand_over(system, state);
    execute(system, state, (fs, kernel), saved.as_ref());
}

/// Whether `selected` names anything to boot, as the legacy code checks;
/// logs what it names, or an error when it names nothing.
fn names_a_kernel(selected: &Kernel) -> bool {
    let Kernel {
        dataset: fs,
        kernel,
        initramfs,
    } = selected;
    if fs.is_empty() && kernel.is_empty() && initramfs.is_empty() {
        zbm_trace::error!("fs, kernel, initramfs undefined");
        return false;
    }
    zbm_trace::debug!(
        "fs: {fs}, kernel: {kernel}, initramfs: {initramfs}";
        %fs, %kernel, %initramfs
    );
    true
}

/// The variables the hooks of the boot get: those of `settings`, then the
/// boot environment, kernel and initramfs of `selected`.
fn hook_env<'a>(settings: &'a BootSettings, selected: &'a Kernel) -> Vec<(&'a str, &'a str)> {
    let mut hook_env: Vec<(&str, &str)> = settings
        .env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    hook_env.extend([
        ("ZBM_SELECTED_BE", selected.dataset.as_str()),
        ("ZBM_SELECTED_KERNEL", selected.kernel.as_str()),
        ("ZBM_SELECTED_INITRAMFS", selected.initramfs.as_str()),
    ]);
    hook_env
}

/// Runs the `boot-sel.d` hooks with `hook_env` and the mount point `mnt` of
/// the selected boot environment.
fn run_boot_sel_hooks(
    system: &System<'_>,
    settings: &BootSettings,
    hook_env: &[(&str, &str)],
    mnt: &str,
) {
    let mut boot_sel_env = hook_env.to_vec();
    boot_sel_env.push(("ZBM_SELECTED_MOUNTPOINT", mnt));
    hooks::run_stage(
        system,
        &settings.hooks,
        HookStage::BootSel,
        false,
        &boot_sel_env,
    );
}

/// The command line of `fs` booted with `kernel`, or the one typed for this
/// boot, after logging the messages of its assembly.
fn command_line(
    system: &System<'_>,
    state: &State<'_>,
    settings: &BootSettings,
    fs: &str,
    kernel: &str,
) -> KernelCommandLine {
    let mut messages = Vec::new();
    let zfs = ZfsText::new(system.runner);
    let cli_args = match &settings.command_line {
        Some(line) => commandline::edited_cmdline(
            system.files,
            &zfs,
            state,
            (fs, line),
            settings.set_hostid,
            &mut messages,
        ),
        None => commandline::be_cmdline(
            system.files,
            &zfs,
            state,
            fs,
            kernel,
            settings.set_hostid,
            &mut messages,
        ),
    };
    for message in &messages {
        message.emit();
    }
    cli_args
}

/// The device tree file that `org.zfsbootmenu:devicetree` of `fs` names for
/// `kernel` in the boot environment mounted at `mnt`, if the property is set
/// and the file exists.
fn find_devicetree(
    system: &System<'_>,
    fs: &FilesystemName,
    kernel: &str,
    mnt: &Path,
) -> Option<PathBuf> {
    let dtb_property = zfs_value(system.runner, DEVICETREE.name, fs);
    match dtb_property.as_deref() {
        Some(property) if !property.is_empty() && property != "-" => {
            zbm_trace::debug!("devicetree property set to '{property}'"; %property);
            let candidate = commandline::devicetree_candidate(property, kernel, mnt)?;
            if system.files.is_file(&candidate) {
                zbm_trace::debug!(
                    "devicetree file exists: '{candidate}'";
                    candidate = %candidate.display()
                );
                Some(candidate)
            } else {
                zbm_trace::debug!(
                    "devicetree file missing: '{candidate}'";
                    candidate = %candidate.display()
                );
                None
            }
        }
        _ => {
            zbm_trace::debug!("no devicetree property set");
            None
        }
    }
}

/// Measures `load` into the TPM, if there is one (ROADMAP.md SE10), and
/// loads the copies that went into it, or the boot environment's files
/// when nothing was measured, as [`load_kernel`] does; the copies go once
/// they are loaded, or failed to load.
fn load_measured(
    system: &System<'_>,
    state: &State<'_>,
    load: &KexecLoad<'_>,
    mnt: &Path,
) -> Option<String> {
    let copies = measured::measure(system, state, load);
    let measured = KexecLoad {
        copies: copies.as_ref(),
        ..*load
    };
    let loaded = load_kernel(system, &measured, mnt);
    if let Some(copies) = &copies {
        copies.remove(system);
    }
    loaded
}

/// Loads the kernel and initramfs of `load` into memory, with a second try
/// when the first fails, and returns what `kexec` printed. When both tries
/// fail, unmounts the boot environment mounted at `mnt`, tells the user and
/// returns `None`.
fn load_kernel(system: &System<'_>, load: &KexecLoad<'_>, mnt: &Path) -> Option<String> {
    let mut output = String::new();
    // Lockdown or Secure Boot: the kernel checks what it boots, and loads
    // nothing it refuses (ROADMAP.md, SE10).
    super::mounts::mount_securityfs(system);
    let checked = kexec::checked_boot(system.files);
    for _ in 0..2 {
        let (ok, text) = substituted(load.run(system.kernel, system.runner, checked));
        if ok {
            return Some(text);
        }
        output = text;
    }
    let KexecLoad {
        mnt: mnt_text,
        kernel,
        initramfs,
        ..
    } = load;
    zbm_trace::error!(
        "unable to load {mnt_text}{kernel} and {mnt_text}{initramfs} into memory";
        %mnt_text, %kernel, %initramfs
    );
    zbm_trace::error!("{output}"; %output);
    let _ = recursive_umount(system, mnt);
    TimedPrompt::new()
        .delay(10)
        .message(colorize(
            "red",
            "Unable to load kernel or initramfs into memory",
        ))
        .message(colorize(
            "orange",
            &Printable(&format!("{mnt_text}{kernel}")).to_string(),
        ))
        .message(colorize(
            "orange",
            &Printable(&format!("{mnt_text}{initramfs}")).to_string(),
        ))
        .run(system);
    None
}

/// Logs what `load` loaded and the `output` of `kexec`, when debugging.
fn log_loaded(load: &KexecLoad<'_>, output: &str) {
    let KexecLoad {
        mnt,
        kernel,
        initramfs,
        root,
        args,
        ..
    } = load;
    // `if zdebug; then`: the test itself writes an empty line.
    if zbm_trace::enabled(Level::Debug) {
        zbm_trace::debug!("");
        zbm_trace::debug!(
            "loaded {mnt}{kernel} and {mnt}{initramfs} into memory";
            %mnt, %kernel, %initramfs
        );
        zbm_trace::debug!("kernel command line: '{root} {args}'"; %root, %args);
        zbm_trace::debug!("{output}"; %output);
    }
}

/// Prepares the machine for the loaded kernel: unmounts the boot environment
/// mounted at `mnt`, exports the pools imported read-write and runs the
/// `teardown.d` hooks with `hook_env`.
fn tear_down(system: &System<'_>, settings: &BootSettings, hook_env: &[(&str, &str)], mnt: &Path) {
    umount(system, mnt);
    export_writable_pools(system.runner);
    hooks::run_stage(
        system,
        &settings.hooks,
        HookStage::Teardown,
        false,
        hook_env,
    );
}

/// Saves the logs of this boot to `destination`, if given, once the kernel
/// is loaded and before the teardown hooks, which may unbind the device the
/// partition is on; a failure is logged and the boot goes on.
pub fn save_logs(
    system: &System<'_>,
    state: &State<'_>,
    destination: Option<&Destination>,
) -> Option<Saved> {
    match saved_logs::save(system, state.base(), destination?) {
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

/// Writes the kernel log and the journal again into the logs `saved` for
/// this boot, if any, so that they keep how the GPUs were handed back, which
/// comes after them and which the next kernel would not see. A failure is
/// logged.
fn keep_hand_back(system: &System<'_>, state: &State<'_>, saved: Option<&Saved>) {
    let Some(saved) = saved else {
        return;
    };
    if let Err(error) = saved_logs::update_logs(system, state.base(), saved) {
        zbm_trace::warn!(
            "unable to add the GPUs' hand-back to the saved logs: {error}";
            error = %chain(&error), error_code = %error.code()
        );
    }
}

/// Has the console on the display, if one runs, hand the display, the
/// keyboards and the virtual terminal back, so that the booted kernel takes
/// them over as it would from the kernel console.
fn hand_over(system: &System<'_>, state: &State<'_>) {
    if state.console_pid().is_some() {
        let _ = programs::release_console(system.runner);
    }
}

/// Has the console that handed the display over take it again when
/// `kexec` failed, to show the error and the menu after it.
fn take_back(system: &System<'_>, state: &State<'_>) {
    if state.released_console_pid().is_some() {
        let _ = programs::reattach_console(system.runner);
    }
}

/// The kernel's file that drops clean caches.
const DROP_CACHES: &str = "/proc/sys/vm/drop_caches";

/// The last step before `kexec` (ROADMAP.md #1), which leaves the memory of
/// running processes and of the kernel as it is: removes the key files
/// cached from key sources, and drops the clean page cache, which may hold
/// key files read from key devices, so that the kernel frees those pages,
/// and zeroes them with `init_on_free=1`; and warns when this process still
/// holds secrets.
pub fn wipe_before_kexec(system: &System<'_>, state: &State<'_>) {
    crate::keys::clear_key_caches(system.files, state);
    if let Err(error) = system.files.write(Path::new(DROP_CACHES), b"3\n") {
        zbm_trace::debug!("unable to drop the page cache: {error}"; %error);
    }
    let live = zbm_platform::secret::live_secrets();
    if live > 0 {
        zbm_trace::warn!("{live} secrets are still in memory before kexec"; %live);
    }
}

/// Tells the user which kernel (`kernel` of `fs`) boots and executes it,
/// after handing the GPUs back, and keeping how in the logs `saved` for
/// this boot. Returns only if that fails, after telling the user.
fn execute(
    system: &System<'_>,
    state: &State<'_>,
    (fs, kernel): (&str, &str),
    saved: Option<&Saved>,
) {
    wipe_before_kexec(system, state);
    let booting = format!(
        "\nBooting {} for {} ...\n\n",
        colorize("yellow", &Printable(kernel).to_string()),
        colorize("cyan", &Printable(fs).to_string())
    );
    let _ = system.console.write(booting.as_bytes());
    // The GPUs last, as a display shows nothing once its GPU is handed back
    let gpus = graphics::hand_back(system, Path::new("/"));
    if !gpus.is_empty() {
        keep_hand_back(system, state, saved);
    }

    let (_, output) = substituted(kexec::execute(system.kernel, system.runner));
    // Only a failed kexec returns.
    graphics::take_back(system, Path::new("/"), &gpus);
    take_back(system, state);
    zbm_trace::error!("kexec -e -i failed!");
    zbm_trace::error!("{output}"; %output);
    TimedPrompt::new()
        .delay(10)
        .message(colorize(
            "red",
            &format!("kexec run of {} failed!", Printable(kernel)),
        ))
        .run(system);
}

#[cfg(test)]
mod tests;
