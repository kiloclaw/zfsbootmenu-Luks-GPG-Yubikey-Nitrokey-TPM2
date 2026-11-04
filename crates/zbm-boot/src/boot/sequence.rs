//! The boot sequence: from a selected kernel to running it with `kexec`,
//! through the hooks, the command line, the device tree and the teardown of
//! mounts and pools.

use std::path::{Path, PathBuf};
use zbm_types::code::{Coded as _, chain};

use zbm_config::spec::properties::DEVICETREE;
use zbm_platform::Context;
use zbm_platform::interaction::{Interaction, PromptRequest, Subject};
use zbm_state::State;
use zbm_tools::{kexec, programs};
use zbm_trace::Level;
use zbm_types::{
    DatasetName, FilesystemName, ImportPolicy, KernelCommandLine, Printable, RetryDelay,
};
use zbm_zfs::{Stderr, ZfsText};

use zbm_bootenv::hooks::{self, HookEnv, StageEnv, Variables};
use zbm_bootenv::kernels::Kernel;
use zbm_bootenv::keys::{self, BootPath, KeyLoad};
use zbm_bootenv::{Noise, commandline, mount_zfs, recursive_umount, umount};

use super::measured;
use super::zfs::{export_writable_pools, substituted};
use crate::graphics;
use crate::load::KexecLoad;
use crate::recovery::Recovery;
use crate::saved_logs::{self, Destination, Saved};
use zbm_platform::color::colorize;

/// What booting needs besides the kernel.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootSettings {
    /// `zbm_set_hostid`: replace the host ID on the command line.
    pub set_hostid: bool,
    /// Where the hook directories are.
    pub hooks: PathBuf,
    /// The variables every hook of the boot gets: those of
    /// `/etc/zfsbootmenu.conf`, and `BOOTFS` (ADR 0017, point 7).
    pub env: Variables,
    /// `zbm.save_logs`: where the logs of this boot are saved before the
    /// kernel starts, if anywhere (ROADMAP.md #9).
    pub save_logs: Option<Destination>,
    /// A command line typed in the menu for this boot only, in place of the
    /// boot environment's `org.zfsbootmenu:commandline`.
    pub command_line: Option<String>,
    /// `zbm.import_policy`: a read/write import forces (`-f`) with `force`.
    pub import_policy: ImportPolicy,
}

impl BootSettings {
    /// These settings with `zbm_set_hostid` set to `set_hostid`.
    #[must_use]
    pub fn set_hostid(self, set_hostid: bool) -> BootSettings {
        BootSettings { set_hostid, ..self }
    }

    /// These settings with the hook directories below `hooks`.
    #[must_use]
    pub fn hooks(self, hooks: impl Into<PathBuf>) -> BootSettings {
        BootSettings {
            hooks: hooks.into(),
            ..self
        }
    }

    /// These settings, saving the logs of the boot to `save_logs`, if given.
    #[must_use]
    pub fn save_logs(self, save_logs: Option<Destination>) -> BootSettings {
        BootSettings { save_logs, ..self }
    }

    /// These settings with the import policy `import_policy`.
    #[must_use]
    pub fn import_policy(self, import_policy: ImportPolicy) -> BootSettings {
        BootSettings {
            import_policy,
            ..self
        }
    }
}

impl Default for BootSettings {
    fn default() -> BootSettings {
        BootSettings {
            set_hostid: true,
            hooks: PathBuf::from(hooks::HOOKS),
            env: Variables::default(),
            save_logs: None,
            command_line: None,
            import_policy: ImportPolicy::default(),
        }
    }
}

/// Loads the key of `fs` if it is locked, with key sources, asking
/// `interaction`. The caller has loaded it already, so this loads it for a
/// boot chosen in the menu; a failure shows when mounting.
fn load_key(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    state: &State<'_>,
    settings: &BootSettings,
    fs: &DatasetName,
) {
    let how = KeyLoad::new(&settings.hooks, &settings.env, BootPath::Menu);
    let _ = keys::load_key(context, interaction, state, &how, fs);
}

/// Loads the key of `fs` and mounts it; the filesystem and its mount point,
/// or `None` when it cannot be mounted. A kernel list line whose dataset is
/// no name fails to mount.
fn load_and_mount(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    state: &State<'_>,
    settings: &BootSettings,
    fs: &str,
) -> Option<(FilesystemName, PathBuf)> {
    let filesystem = fs.parse::<FilesystemName>().ok();
    if let Some(filesystem) = &filesystem {
        load_key(context, interaction, state, settings, filesystem.dataset());
    }
    let filesystem = filesystem?;
    // Mounted until the teardown, or the recursive unmount of a failure.
    let mnt = mount_zfs(context, state, &filesystem, Noise::Shown)?.leave();
    Some((filesystem, mnt))
}

/// Boots `selected`: mounts it, runs the `boot-sel.d` hooks, loads the kernel
/// with its command line, unmounts it, exports the pools imported read-write,
/// runs the `teardown.d` hooks, has the console on the display hand the
/// display back, and executes the kernel. Returns only if that fails, after
/// telling the user through `interaction`, which also asks for a key. The
/// screen is the caller's: the plain console clears it before a boot.
pub fn kexec_kernel(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    state: &State<'_>,
    settings: &BootSettings,
    selected: &Kernel,
    recovery: &dyn Recovery,
) {
    if !names_a_kernel(selected) {
        return;
    }
    let (fs, kernel, initramfs) = (&selected.dataset, &selected.kernel, &selected.initramfs);
    let Some((filesystem, mnt)) = load_and_mount(context, interaction, state, settings, fs) else {
        recovery.emergency_shell(&format!("unable to mount {}", colorize("cyan", fs)));
        return;
    };
    let mnt_text = mnt.to_string_lossy().into_owned();
    run_boot_sel_hooks(context, settings, selected, &mnt_text);

    let cli_args = command_line(context, state, settings, fs, kernel);
    let root = match commandline::boot_root(context, &filesystem, &mnt) {
        Ok(root) => root,
        Err(root_prefix) => {
            zbm_trace::error!(
                root_prefix,
                fs,
                "the root argument is not one kernel argument"
            );
            let _ = recursive_umount(context, &mnt);
            recovery.emergency_shell(&format!("unable to boot {}", colorize("cyan", fs)));
            return;
        }
    };
    let dtb = find_devicetree(context, &filesystem, kernel, &mnt);
    let load = KexecLoad {
        mnt: &mnt_text,
        kernel,
        initramfs,
        dtb: dtb.as_deref(),
        root: &root,
        args: &cli_args,
        copies: None,
    };
    let Some(output) = load_measured(context, interaction, state, &load, &mnt) else {
        return;
    };
    log_loaded(&load, &output);

    // The logs are saved before the teardown hooks, which may take away
    // the partition they go to (`90-xhci-unbind` and an ESP on USB).
    let saved = save_logs(context, state, settings.save_logs.as_ref());
    tear_down(context, settings, selected, &mnt);
    hand_over(context, state);
    execute(context, interaction, state, (fs, kernel), saved.as_ref());
}

/// Whether `selected` names anything to boot, as the legacy code checks;
/// logs what it names, or an error when it names nothing.
fn names_a_kernel(selected: &Kernel) -> bool {
    let Kernel {
        dataset: fs,
        kernel,
        initramfs,
        ..
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

/// Runs the `boot-sel.d` hooks for `selected`, whose boot environment is
/// mounted at `mnt`.
fn run_boot_sel_hooks(
    context: &Context<'_>,
    settings: &BootSettings,
    selected: &Kernel,
    mnt: &str,
) {
    let env = HookEnv {
        variables: &settings.env,
        stage: StageEnv::BootSel {
            selected,
            mountpoint: mnt,
        },
    };
    hooks::run_stage(context, &settings.hooks, &env, false);
}

/// The command line of `fs` booted with `kernel`, or the one typed for this
/// boot, after logging the messages of its assembly.
fn command_line(
    context: &Context<'_>,
    state: &State<'_>,
    settings: &BootSettings,
    fs: &str,
    kernel: &str,
) -> KernelCommandLine {
    let mut messages = Vec::new();
    let zfs = ZfsText::new(context.runner());
    let cli_args = match &settings.command_line {
        Some(line) => commandline::edited_cmdline(
            context.files(),
            &zfs,
            state,
            (fs, line),
            settings.set_hostid,
            &mut messages,
        ),
        None => commandline::be_cmdline(
            context.files(),
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
    context: &Context<'_>,
    fs: &FilesystemName,
    kernel: &str,
    mnt: &Path,
) -> Option<PathBuf> {
    let dtb_property = ZfsText::new(context.runner()).value(fs, DEVICETREE.name, Stderr::Hidden);
    match dtb_property.as_deref() {
        Some(property) if !property.is_empty() && property != "-" => {
            zbm_trace::debug!("devicetree property set to '{property}'"; %property);
            let candidate = commandline::devicetree_candidate(property, kernel, mnt)?;
            if context.files().is_file(&candidate) {
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
    context: &Context<'_>,
    interaction: &dyn Interaction,
    state: &State<'_>,
    load: &KexecLoad<'_>,
    mnt: &Path,
) -> Option<String> {
    let copies = measured::measure(context, state, load);
    let measured = KexecLoad {
        copies: copies.as_ref(),
        ..*load
    };
    let loaded = load_kernel(context, interaction, &measured, mnt);
    if let Some(copies) = &copies {
        copies.remove(context);
    }
    loaded
}

/// Loads the kernel and initramfs of `load` into memory, with a second try
/// when the first fails, and returns what `kexec` printed. When both tries
/// fail, unmounts the boot environment mounted at `mnt`, tells the user
/// through `interaction` and returns `None`.
fn load_kernel(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    load: &KexecLoad<'_>,
    mnt: &Path,
) -> Option<String> {
    let mut output = String::new();
    // Lockdown or Secure Boot: the kernel checks what it boots, and loads
    // nothing it refuses (ROADMAP.md, SE10).
    super::mounts::mount_securityfs(context);
    let checked = kexec::checked_boot(context.files());
    for _ in 0..2 {
        let (ok, text) = substituted(load.run(context.kernel(), context.runner(), checked));
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
    let _ = recursive_umount(context, mnt);
    let _ = interaction.ask(&failed_boot(
        Subject::LoadFailed {
            kernel: Printable(&format!("{mnt_text}{kernel}")).to_string(),
            initramfs: Printable(&format!("{mnt_text}{initramfs}")).to_string(),
        },
        "Unable to load kernel or initramfs into memory".to_owned(),
    ));
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
/// `teardown.d` hooks for `selected`.
fn tear_down(context: &Context<'_>, settings: &BootSettings, selected: &Kernel, mnt: &Path) {
    umount(context, mnt);
    export_writable_pools(context.runner());
    let env = HookEnv {
        variables: &settings.env,
        stage: StageEnv::Teardown { selected },
    };
    hooks::run_stage(context, &settings.hooks, &env, false);
}

/// Saves the logs of this boot to `destination`, if given, once the kernel
/// is loaded and before the teardown hooks, which may unbind the device the
/// partition is on; a failure is logged and the boot goes on.
pub(crate) fn save_logs(
    context: &Context<'_>,
    state: &State<'_>,
    destination: Option<&Destination>,
) -> Option<Saved> {
    match saved_logs::save(context, state.base(), destination?) {
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
fn keep_hand_back(context: &Context<'_>, state: &State<'_>, saved: Option<&Saved>) {
    let Some(saved) = saved else {
        return;
    };
    if let Err(error) = saved_logs::update_logs(context, state.base(), saved) {
        zbm_trace::warn!(
            "unable to add the GPUs' hand-back to the saved logs: {error}";
            error = %chain(&error), error_code = %error.code()
        );
    }
}

/// Has the console on the display, if one runs, hand the display, the
/// keyboards and the virtual terminal back, so that the booted kernel takes
/// them over as it would from the kernel console.
fn hand_over(context: &Context<'_>, state: &State<'_>) {
    if state.console_pid().is_some() {
        let _ = programs::release_console(context.runner());
    }
}

/// Has the console that handed the display over take it again when
/// `kexec` failed, to show the error and the menu after it.
fn take_back(context: &Context<'_>, state: &State<'_>) {
    if state.released_console_pid().is_some() {
        let _ = programs::reattach_console(context.runner());
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
pub(crate) fn wipe_before_kexec(context: &Context<'_>, state: &State<'_>) {
    keys::clear_key_caches(context.files(), state);
    if let Err(error) = context.files().write(Path::new(DROP_CACHES), b"3\n") {
        zbm_trace::debug!("unable to drop the page cache: {error}"; %error);
    }
    let live = zbm_platform::secret::live_secrets();
    if live > 0 {
        zbm_trace::warn!("{live} secrets are still in memory before kexec"; %live);
    }
}

/// Tells the user, through `interaction`, which kernel (`kernel` of `fs`)
/// boots and executes it, after handing the GPUs back, and keeping how in
/// the logs `saved` for this boot. Returns only if that fails, after
/// telling the user.
fn execute(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    state: &State<'_>,
    (fs, kernel): (&str, &str),
    saved: Option<&Saved>,
) {
    wipe_before_kexec(context, state);
    let kernel = Printable(kernel).to_string();
    interaction.booting(&kernel, &Printable(fs).to_string());
    // The GPUs last, as a display shows nothing once its GPU is handed back
    let gpus = graphics::hand_back(context, Path::new("/"));
    if !gpus.is_empty() {
        keep_hand_back(context, state, saved);
    }

    let (_, output) = substituted(kexec::execute(context.kernel(), context.runner()));
    // Only a failed kexec returns.
    graphics::take_back(context, Path::new("/"), &gpus);
    take_back(context, state);
    zbm_trace::error!("kexec -e -i failed!");
    zbm_trace::error!("{output}"; %output);
    let message = format!("kexec run of {kernel} failed!");
    let _ = interaction.ask(&failed_boot(Subject::KexecFailed { kernel }, message));
}

/// The prompt of a boot that failed, about `subject`, with `message`: ten
/// seconds, then the menu again.
fn failed_boot(subject: Subject, message: String) -> PromptRequest {
    PromptRequest {
        subject,
        delay: RetryDelay::new(10),
        message,
        on_return: String::new(),
        on_escape: String::new(),
    }
}

#[cfg(test)]
mod tests;
