//! The boot sequence: from a selected kernel to running it with `kexec`,
//! through the hooks, the command line, the device tree and the teardown of
//! mounts and pools.

use std::path::{Path, PathBuf};

use zbm_config::spec::properties::{DEVICETREE, ROOTPREFIX};
use zbm_platform::System;
use zbm_state::State;
use zbm_tools::{kexec, programs};
use zbm_trace::Level;
use zbm_types::HookStage;
use zbm_zfs::ZfsCommand;

use super::mounts::{Noise, mount_zfs, recursive_umount, umount};
use super::zfs::{export_writable_pools, locked_root, substituted, zfs_value};
use crate::commandline::{self, KexecLoad};
use crate::hooks;
use crate::kernels::Kernel;
use crate::prompt::{TimedPrompt, colorize, tput};
use crate::recovery::Recovery;
use crate::saved_logs::{self, Destination};
use crate::support::{self, Unsupported};

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
}

impl Default for BootSettings {
    fn default() -> BootSettings {
        BootSettings {
            set_hostid: true,
            hooks: PathBuf::from(hooks::HOOKS),
            env: Vec::new(),
            save_logs: None,
        }
    }
}

/// The root prefix of `fs` mounted at `mnt`, with the messages of
/// `find_root_prefix`.
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

/// Boots `selected` as `kexec_kernel` does: mounts it, runs the `boot-sel.d`
/// hooks, loads the kernel with its command line, unmounts it, exports the
/// pools imported read-write, runs the `teardown.d` hooks, has the console
/// on the display hand the display back, and executes the kernel. Returns
/// only if that fails, after telling the user.
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

    // load_key: keys are not loaded yet, so a locked boot environment stops here.
    zbm_trace::debug!("fs set to {fs}");
    if let Some(root) = locked_root(system.runner, fs) {
        zbm_trace::error!("{}", Unsupported::LockedEncryptionRoot(root).to_string());
        recovery.emergency_shell(support::MESSAGE);
        return;
    }

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

    tear_down(system, settings, &hook_env, &mnt);
    save_logs(system, state, settings.save_logs.as_ref());
    hand_over(system, state);
    execute(system, fs, kernel);
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

/// The command line of `fs` booted with `kernel`, after logging the messages
/// of its assembly.
fn command_line(
    system: &System<'_>,
    state: &State<'_>,
    settings: &BootSettings,
    fs: &str,
    kernel: &str,
) -> String {
    let mut messages = Vec::new();
    let zfs = ZfsCommand::new(system.runner);
    let cli_args = commandline::be_cmdline(
        system.files,
        &zfs,
        state,
        fs,
        kernel,
        settings.set_hostid,
        &mut messages,
    );
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

/// Saves the logs of this boot to `destination`, if given, last of all but
/// the hand-over, so that they hold the teardown; a failure is logged and
/// the boot goes on.
pub fn save_logs(system: &System<'_>, state: &State<'_>, destination: Option<&Destination>) {
    let Some(destination) = destination else {
        return;
    };
    match saved_logs::save(system, state.base(), destination) {
        Ok(saved) => zbm_trace::info!("saved the logs on {} in {}", saved.device, saved.directory),
        Err(error) => zbm_trace::warn!("unable to save the logs: {error}"),
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

/// Tells the user which kernel boots and executes it. Returns only if that
/// fails, after telling the user.
fn execute(system: &System<'_>, fs: &str, kernel: &str) {
    let booting = format!(
        "\nBooting {} for {} ...\n\n",
        colorize("yellow", kernel),
        colorize("cyan", fs)
    );
    let _ = system.console.write(booting.as_bytes());

    let (_, output) = substituted(kexec::execute(system.runner));
    // Only a failed kexec returns.
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
}
