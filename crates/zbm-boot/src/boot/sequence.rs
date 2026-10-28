//! The boot sequence: from a selected kernel to running it with `kexec`,
//! through the hooks, the command line, the device tree and the teardown of
//! mounts and pools.

use std::path::{Path, PathBuf};

use zbm_config::spec::properties::{DEVICETREE, ROOTPREFIX};
use zbm_platform::System;
use zbm_state::State;
use zbm_tools::{kexec, programs};
use zbm_trace::Level;
use zbm_types::{HookStage, ImportPolicy};
use zbm_zfs::ZfsCommand;

use super::mounts::{Noise, mount_zfs, recursive_umount, umount};
use super::zfs::{export_writable_pools, substituted, zfs_value};
use crate::commandline::{self, KexecLoad};
use crate::graphics;
use crate::hooks;
use crate::kernels::Kernel;
use crate::keys::{self, BootPath, Clear, KeyLoad};
use crate::prompt::{TimedPrompt, colorize, tput};
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
fn find_root_prefix(system: &System<'_>, fs: &str, mnt: &Path) -> String {
    zbm_trace::debug!("zfsbe_fs set to {fs}");
    zbm_trace::debug!("zfsbe_mnt set to {}", mnt.display());
    if let Some(prefix) = zfs_value(system.runner, ROOTPREFIX.name, fs)
        && prefix != "-"
    {
        zbm_trace::debug!("using org.zfsbootmenu:rootprefix: {prefix}");
        return prefix;
    }
    if let Some(prefix) = commandline::os_release_prefix(system.files, mnt) {
        zbm_trace::debug!("using os-release: {prefix}");
        return prefix;
    }
    zbm_trace::debug!("using default");
    "root=zfs:".to_owned()
}

/// Loads the key of `fs` if it is locked, with key sources and after
/// clearing the screen. The countdown has loaded it already, so this loads
/// it for a boot chosen in the menu; a failure shows when mounting.
fn load_key(system: &System<'_>, state: &State<'_>, settings: &BootSettings, fs: &str) {
    let how = KeyLoad {
        hooks: &settings.hooks,
        path: BootPath::Menu,
        clear: Clear::Yes,
    };
    let _ = keys::load_key(system, state, &how, fs);
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
    recovery: &mut dyn Recovery,
) {
    let Kernel {
        dataset: fs,
        kernel,
        initramfs,
    } = selected;
    if fs.is_empty() && kernel.is_empty() && initramfs.is_empty() {
        zbm_trace::error!("fs, kernel, initramfs undefined");
        return;
    }
    zbm_trace::debug!("fs: {fs}, kernel: {kernel}, initramfs: {initramfs}");

    load_key(system, state, settings, fs);

    tput(system, &["cnorm"]);
    tput(system, &["clear"]);

    let Some(mnt) = mount_zfs(system, state, fs, Noise::Shown) else {
        recovery.emergency_shell(&format!("unable to mount {}", colorize("cyan", fs)));
        return;
    };
    let mnt_text = mnt.to_string_lossy().into_owned();
    let hook_env = hook_env(settings, selected);
    run_boot_sel_hooks(system, settings, &hook_env, &mnt_text);

    let cli_args = command_line(system, state, settings, fs, kernel);
    let root_prefix = find_root_prefix(system, fs, &mnt);
    let dtb = find_devicetree(system, fs, kernel, &mnt);
    let load = KexecLoad {
        mnt: &mnt_text,
        dataset: fs,
        kernel,
        initramfs,
        dtb: dtb.as_deref(),
        root_prefix: &root_prefix,
        args: &cli_args,
    };
    let Some(output) = load_kernel(system, &load, &mnt) else {
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
) -> String {
    let mut messages = Vec::new();
    let zfs = ZfsCommand::new(system.runner);
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
fn find_devicetree(system: &System<'_>, fs: &str, kernel: &str, mnt: &Path) -> Option<PathBuf> {
    let dtb_property = zfs_value(system.runner, DEVICETREE.name, fs);
    match dtb_property.as_deref() {
        Some(property) if !property.is_empty() && property != "-" => {
            zbm_trace::debug!("devicetree property set to '{property}'");
            let candidate = commandline::devicetree_candidate(property, kernel, mnt)?;
            if system.files.is_file(&candidate) {
                zbm_trace::debug!("devicetree file exists: '{}'", candidate.display());
                Some(candidate)
            } else {
                zbm_trace::debug!("devicetree file missing: '{}'", candidate.display());
                None
            }
        }
        _ => {
            zbm_trace::debug!("no devicetree property set");
            None
        }
    }
}

/// Loads the kernel and initramfs of `load` into memory, with a second try
/// when the first fails, and returns what `kexec` printed. When both tries
/// fail, unmounts the boot environment mounted at `mnt`, tells the user and
/// returns `None`.
fn load_kernel(system: &System<'_>, load: &KexecLoad<'_>, mnt: &Path) -> Option<String> {
    let mut output = String::new();
    for _ in 0..2 {
        let (ok, text) = substituted(load.run(system.runner));
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
    zbm_trace::error!("unable to load {mnt_text}{kernel} and {mnt_text}{initramfs} into memory");
    zbm_trace::error!("{}", output);
    let _ = recursive_umount(system, mnt);
    TimedPrompt::new()
        .delay(10)
        .message(colorize(
            "red",
            "Unable to load kernel or initramfs into memory",
        ))
        .message(colorize("orange", &format!("{mnt_text}{kernel}")))
        .message(colorize("orange", &format!("{mnt_text}{initramfs}")))
        .run(system);
    None
}

/// Logs what `load` loaded and the `output` of `kexec`, when debugging.
fn log_loaded(load: &KexecLoad<'_>, output: &str) {
    let KexecLoad {
        mnt,
        dataset,
        kernel,
        initramfs,
        root_prefix,
        args,
        ..
    } = load;
    // `if zdebug; then`: the test itself writes an empty line.
    if zbm_trace::enabled(Level::Debug) {
        zbm_trace::debug!("");
        zbm_trace::debug!("loaded {mnt}{kernel} and {mnt}{initramfs} into memory");
        zbm_trace::debug!("kernel command line: '{root_prefix}{dataset} {args}'");
        zbm_trace::debug!("{}", output);
    }
}

/// Prepares the machine for the loaded kernel: unmounts the boot environment
/// mounted at `mnt`, exports the pools imported read-write and runs the
/// `teardown.d` hooks with `hook_env`.
fn tear_down(system: &System<'_>, settings: &BootSettings, hook_env: &[(&str, &str)], mnt: &Path) {
    umount(system.runner, mnt);
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
            zbm_trace::info!("saved the logs on {} in {}", saved.device, saved.directory);
            Some(saved)
        }
        Err(error) => {
            zbm_trace::warn!("unable to save the logs: {error}");
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
        zbm_trace::warn!("unable to add the GPUs' hand-back to the saved logs: {error}");
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
        zbm_trace::debug!("unable to drop the page cache: {error}");
    }
    let live = zbm_platform::secret::live_secrets();
    if live > 0 {
        zbm_trace::warn!("{live} secrets are still in memory before kexec");
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
        colorize("yellow", kernel),
        colorize("cyan", fs)
    );
    let _ = system.console.write(booting.as_bytes());
    // The GPUs last, as a display shows nothing once its GPU is handed back
    let gpus = graphics::hand_back(system, Path::new("/"));
    if !gpus.is_empty() {
        keep_hand_back(system, state, saved);
    }

    let (_, output) = substituted(kexec::execute(system.runner));
    // Only a failed kexec returns.
    graphics::take_back(system, Path::new("/"), &gpus);
    take_back(system, state);
    zbm_trace::error!("kexec -e -i failed!");
    zbm_trace::error!("{}", output);
    TimedPrompt::new()
        .delay(10)
        .message(colorize("red", &format!("kexec run of {kernel} failed!")))
        .run(system);
}

#[cfg(test)]
mod tests {
    use zbm_platform::Files as _;
    use zbm_testkit::fakes::FakeSystem;

    use super::*;

    #[test]
    fn the_console_on_the_display_hands_it_back_before_kexec() {
        let fake = FakeSystem::new();
        let system = fake.system();
        let state = State::new(system.files, "/zfsbootmenu");

        hand_over(&system, &state);
        assert!(fake.runner.calls().is_empty());

        state.create().unwrap();
        state.set_console(42).unwrap();
        hand_over(&system, &state);
        let calls: Vec<String> = fake
            .runner
            .calls()
            .iter()
            .map(|call| call.display())
            .collect();
        assert_eq!(calls, ["/libexec/zbm console release"]);
    }

    #[test]
    fn after_a_failed_kexec_the_console_takes_the_display_again() {
        let fake = FakeSystem::new();
        let system = fake.system();
        let state = State::new(system.files, "/zfsbootmenu");

        take_back(&system, &state);
        assert!(fake.runner.calls().is_empty());

        state.create().unwrap();
        state.set_released_console(42).unwrap();
        take_back(&system, &state);
        let calls: Vec<String> = fake
            .runner
            .calls()
            .iter()
            .map(|call| call.display())
            .collect();
        assert_eq!(calls, ["/libexec/zbm console reattach"]);
    }

    #[test]
    fn the_logs_are_saved_or_the_failure_logged() {
        let fake = FakeSystem::new();
        let system = fake.system();
        let state = State::new(system.files, "/zfsbootmenu");
        fake.runner.respond(
            "lsblk -rnpo NAME,PARTTYPE",
            0,
            "/dev/vda1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n",
        );

        save_logs(&system, &state, None);
        assert!(fake.runner.lines().is_empty());
        save_logs(&system, &state, Some(&Destination::Esp));
        let none = Destination::Device("LABEL=none".to_owned());
        save_logs(&system, &state, Some(&none));

        let lines = fake.runner.lines();
        assert!(
            lines.contains(&"mount -t vfat -o rw,noatime /dev/vda1 /run/zbm-saved-logs".to_owned()),
            "{lines:?}"
        );
        assert!(fake.files.exists(std::path::Path::new(
            "/run/zbm-saved-logs/EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z/kernel.log"
        )));
    }

    #[test]
    fn the_hand_back_goes_into_the_logs_saved_for_the_boot() {
        let fake = FakeSystem::new();
        // The kernel log as reading `/dev/kmsg` gives it.
        fake.files.file(
            "/dev/kmsg",
            "6,1,42000000,-;ZFSBootMenu: graphics: handed back\n",
        );
        fake.files
            .dir("/run/zbm-saved-logs/EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z");
        let system = fake.system();
        let _log =
            zbm_platform::KernelLog::new(&fake.files, "7", std::path::Path::new("/zfsbootmenu"), 1)
                .install();

        let state = State::new(system.files, "/zfsbootmenu");
        keep_hand_back(&system, &state, None);
        assert!(fake.runner.lines().is_empty());

        let saved = Saved {
            device: "/dev/vda1".to_owned(),
            directory: "EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z".to_owned(),
            files: Vec::new(),
        };
        keep_hand_back(&system, &state, Some(&saved));
        assert!(
            fake.files
                .text("/run/zbm-saved-logs/EFI/zfsbootmenu/logs/2026-01-01T00-00-00Z/kernel.log")
                .is_some_and(|log| log.contains("handed back"))
        );

        fake.runner.respond(
            "mount -t vfat -o rw,noatime /dev/vda1 /run/zbm-saved-logs",
            32,
            "",
        );
        keep_hand_back(&system, &state, Some(&saved));
        assert!(
            fake.kmsg()
                .contains("unable to add the GPUs' hand-back to the saved logs")
        );
    }

    #[test]
    fn the_page_cache_is_dropped_and_live_secrets_named_before_kexec() {
        let fake = FakeSystem::new();
        fake.files.dir("/proc/sys/vm");
        let system = fake.system();
        let state = State::new(&fake.files, "/zfsbootmenu");
        let _log =
            zbm_platform::KernelLog::new(&fake.files, "7", std::path::Path::new("/zfsbootmenu"), 1)
                .install();

        wipe_before_kexec(&system, &state);
        assert_eq!(fake.files.text(DROP_CACHES).as_deref(), Some("3\n"));
        assert!(!fake.kmsg().contains("still in memory"), "{}", fake.kmsg());
        let held = zbm_platform::secret::Secret::copy_from(b"pass");
        wipe_before_kexec(&system, &state);
        assert!(
            fake.kmsg()
                .contains("secrets are still in memory before kexec")
        );
        drop(held);
    }

    #[test]
    fn the_cached_keys_are_removed_before_kexec_but_not_what_they_link_to() {
        let fake = FakeSystem::new();
        fake.files
            .dir("/proc/sys/vm")
            .file("/zfsbootmenu/.keys/pool/keys/a.key", "key")
            .file("/elsewhere/kept", "kept")
            .symlink("/zfsbootmenu/.keys/pool/link", "/elsewhere");
        let system = fake.system();
        let state = State::new(&fake.files, "/zfsbootmenu");

        wipe_before_kexec(&system, &state);

        assert!(
            !fake
                .files
                .exists(std::path::Path::new("/zfsbootmenu/.keys"))
        );
        assert_eq!(fake.files.text("/elsewhere/kept").as_deref(), Some("kept"));
    }
}
