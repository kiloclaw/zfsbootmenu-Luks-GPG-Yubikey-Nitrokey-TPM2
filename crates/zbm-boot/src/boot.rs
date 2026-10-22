//! Booting a boot environment.
//!
//! The Rust counterparts of `mount_zfs`, `find_be_kernels`, `select_kernel`,
//! `kexec_kernel` and the helpers they use (`be_has_encroot`, `be_is_locked`,
//! `is_writable`, `export_pool`, `recursive_umount`, `mount_efivarfs`), for
//! the default boot of the Phase 5 runtime: read-only mounts, no key loading
//! (a locked encryption root is not supported yet).

use std::path::{Path, PathBuf};

use zbm_core::shell::{command_substitution, read_lines};
use zbm_platform::exec::{Command, Stream};
use zbm_platform::{Runner, System};
use zbm_state::State;
use zbm_trace::Level;
use zbm_zfs::ZfsCommand;

use crate::commandline::{self, KexecLoad};
use crate::error::BootError;
use crate::hooks;
use crate::kernels::{self, Kernel};
use crate::prompt::{TimedPrompt, colorize, tput};
use crate::recovery::Recovery;
use crate::support::{self, Unsupported};
use zbm_types::HookStage;

/// The lines `while read -r line ... done <<< "$( command )"` reads.
fn here_lines(output: &str) -> Vec<String> {
    read_lines(&format!("{}\n", command_substitution(output)))
}

/// What `$( command )` gives, and whether the command succeeded.
fn substitution(runner: &dyn Runner, command: &Command) -> (bool, String) {
    match runner.run(command) {
        Ok(output) => (
            output.status.is_success(),
            command_substitution(&output.stdout_text()).to_owned(),
        ),
        Err(error) => (false, error.to_string()),
    }
}

/// `zfs get -H -o value <property> <dataset> 2>/dev/null`, or `None` when it
/// fails.
pub(crate) fn zfs_value(runner: &dyn Runner, property: &str, dataset: &str) -> Option<String> {
    let command = Command::new("zfs")
        .args(["get", "-H", "-o", "value", property, dataset])
        .stderr(Stream::Null);
    let (ok, value) = substitution(runner, &command);
    ok.then_some(value)
}

/// The encryption root of `fs` (a snapshot counts as its filesystem), as
/// `be_has_encroot` finds it.
pub fn encryption_root(runner: &dyn Runner, fs: &str) -> Option<String> {
    // ${1%@*}
    let fs = fs.rsplit_once('@').map_or(fs, |(fs, _)| fs);
    if fs.is_empty() {
        zbm_trace::error!("fs is undefined");
        return None;
    }
    zbm_trace::debug!("fs set to {fs}");
    let pool = fs.split('/').next().unwrap_or(fs);
    let feature = Command::new("zpool")
        .args(["list", "-H", "-o", "feature@encryption", pool])
        .stderr(Stream::Null);
    if substitution(runner, &feature).1 != "active" {
        zbm_trace::debug!("feature@encryption not active on {pool}");
        return None;
    }
    let encroot = zfs_value(runner, "encryptionroot", fs)?;
    zbm_trace::debug!("{fs} encryptionroot property: {encroot}");
    (encroot != "-").then_some(encroot)
}

/// The encryption root of `fs` if its key is not loaded, as `be_is_locked`
/// decides.
pub fn locked_root(runner: &dyn Runner, fs: &str) -> Option<String> {
    if fs.is_empty() {
        zbm_trace::error!("fs is undefined");
        return None;
    }
    zbm_trace::debug!("fs set to {fs}");
    let encroot = encryption_root(runner, fs)?;
    zbm_trace::debug!("{encroot} discovered as encryption root for {fs}");
    let keystatus = Command::new("zfs")
        .args(["get", "-H", "-o", "value", "keystatus", &encroot])
        .merge_stderr();
    let keystatus = substitution(runner, &keystatus).1;
    zbm_trace::debug!("{encroot} keystatus: {keystatus}");
    (keystatus == "unavailable").then_some(encroot)
}

/// Whether `pool` is imported read-write (`is_writable`).
pub fn is_writable(runner: &dyn Runner, pool: &str) -> bool {
    if pool.is_empty() {
        zbm_trace::error!("pool is undefined");
        return false;
    }
    zbm_trace::debug!("pool set to {pool}");
    let command = Command::new("zpool")
        .args(["get", "-H", "-o", "value", "readonly", pool])
        .stderr(Stream::Null);
    let (ok, readonly) = substitution(runner, &command);
    if !ok {
        return false;
    }
    zbm_trace::debug!("{pool} readonly property: {readonly}");
    readonly == "off"
}

/// Exports `pool` (`export_pool`).
///
/// # Errors
///
/// [`BootError::NoPool`] without a pool, [`BootError::Export`] when
/// `zpool export` fails.
pub fn export_pool(runner: &dyn Runner, pool: &str) -> Result<(), BootError> {
    if pool.is_empty() {
        zbm_trace::error!("pool is undefined");
        return Err(BootError::NoPool);
    }
    zbm_trace::debug!("pool set to {pool}");
    let command = Command::new("zpool").args(["export", pool]);
    let code = match runner.run(&command) {
        Ok(output) => output.status.shell_code(),
        Err(_) => 126,
    };
    zbm_trace::debug!("{pool} export process return: {code}");
    if code == 0 {
        Ok(())
    } else {
        Err(BootError::Export {
            pool: pool.to_owned(),
            status: code,
        })
    }
}

/// The state directory of a boot environment (`be_location`), created if
/// needed.
fn be_location(state: &State<'_>, fs: &str) -> PathBuf {
    zbm_trace::debug!("fs set to {fs}");
    match state.environment(fs) {
        Ok(location) => location,
        // `mkdir -p` failed; be_location gives the path all the same.
        Err(_) => {
            let mnt = state.mount_point(fs);
            mnt.parent().map_or(mnt.clone(), Path::to_path_buf)
        }
    }
}

/// How the output of `mount` reaches the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Noise {
    /// Error messages go to the console, as in `kexec_kernel`.
    Shown,
    /// Error messages are captured and dropped, as `$( mount_zfs ... 2>&1 )`
    /// in `find_be_kernels` drops them.
    Hidden,
}

/// Mounts `fs` read-only on its mount point in the state directory, as
/// `mount_zfs` does, and returns the mount point.
pub fn mount_zfs(
    system: &System<'_>,
    state: &State<'_>,
    fs: &str,
    noise: Noise,
) -> Option<PathBuf> {
    if fs.is_empty() {
        zbm_trace::error!("fs is undefined");
        return None;
    }
    if locked_root(system.runner, fs).is_some() {
        zbm_trace::error!("{fs} is locked, unable to mount filesystem");
        zbm_trace::debug!("{fs} is locked, returning");
        return None;
    }
    let mnt = be_location(state, fs).join("mnt");
    let _ = system.files.create_dir_all(&mnt);
    let rwo = "ro";
    let stderr = match noise {
        Noise::Shown => Stream::Inherit,
        Noise::Hidden => Stream::Null,
    };

    let mountpoint = Command::new("zfs")
        .args(["get", "-H", "-o", "value", "mountpoint", fs])
        .stderr(stderr);
    let legacy = substitution(system.runner, &mountpoint).1 == "legacy";
    let mnt_text = mnt.to_string_lossy();
    let options = if legacy || fs.contains('@') {
        zbm_trace::debug!("mounting {fs} at {mnt_text} ({rwo})");
        rwo.to_owned()
    } else {
        zbm_trace::debug!("mounting {fs} at {mnt_text} ({rwo}) with zfsutil");
        zbm_trace::debug!("mount -o zfsutil,{rwo} -t zfs {fs} {mnt_text}");
        format!("zfsutil,{rwo}")
    };
    let mount = Command::new("mount")
        .args(["-o", &options, "-t", "zfs", fs])
        .arg(&mnt)
        .stderr(stderr);
    let code = match system.runner.run(&mount) {
        Ok(output) => output.status.shell_code(),
        Err(_) => 126,
    };
    zbm_trace::debug!("mount return code: {code}");
    (code == 0).then_some(mnt)
}

/// Runs `umount <path>`.
fn umount(runner: &dyn Runner, path: &Path) {
    let command = Command::new("umount").arg(path).stdout(Stream::Inherit);
    let _ = runner.run(&command);
}

/// Finds the kernels of `fs` and records them in its kernel list, as
/// `find_be_kernels` does, with the command line files next to them in the
/// command line cache.
///
/// # Errors
///
/// [`BootError::NoFilesystem`] without a boot environment,
/// [`BootError::Mount`] when it cannot be mounted, [`BootError::NoKernel`]
/// when no kernel can be selected.
pub fn find_kernels(system: &System<'_>, state: &State<'_>, fs: &str) -> Result<(), BootError> {
    if fs.is_empty() {
        zbm_trace::error!("fs is undefined");
        return Err(BootError::NoFilesystem);
    }
    zbm_trace::debug!("fs set to {fs}");
    let Some(mnt) = mount_zfs(system, state, fs, Noise::Hidden) else {
        zbm_trace::error!("unable to mount {fs}");
        return Err(BootError::Mount { fs: fs.to_owned() });
    };
    let mnt_text = mnt.to_string_lossy().into_owned();
    let records = state.kernels(fs);
    let _ = system.files.write(&records, b"");

    // Start the command line cache afresh.
    be_location(state, fs);
    if let Some(cache) = state.kcl_cache(fs, ".")
        && let Some(dir) = cache.parent()
    {
        let _ = system.files.create_dir_all(dir);
        for name in system.files.read_dir(dir).unwrap_or_default() {
            if name.as_encoded_bytes().ends_with(b".kcl")
                && !name.as_encoded_bytes().starts_with(b".")
            {
                let _ = system.files.remove_file(&dir.join(name));
            }
        }
    }

    let mut list = Vec::new();
    for path in kernels::kernel_files(system.files, &mnt) {
        let rest = path.strip_prefix(mnt_text.as_str()).unwrap_or(&path);
        let kpath = format!("/{}", rest.strip_prefix('/').unwrap_or(rest));
        match kernels::find_initramfs(system.files, Path::new(&path)) {
            Some(initramfs) => {
                let initramfs = initramfs.to_string_lossy();
                zbm_trace::debug!(
                    "found kernel: {mnt_text}{kpath}, initramfs {mnt_text}{initramfs}"
                );
                let rest = initramfs
                    .strip_prefix(mnt_text.as_str())
                    .unwrap_or(&initramfs);
                let ipath = format!("/{}", rest.strip_prefix('/').unwrap_or(rest));
                let kernel = Kernel {
                    dataset: fs.to_owned(),
                    kernel: kpath.clone(),
                    initramfs: ipath,
                };
                list.extend_from_slice(kernel.line().as_bytes());
            }
            None => {
                zbm_trace::debug!("kernel {mnt_text}{kpath} has no initramfs");
            }
        }
        let kcl = PathBuf::from(format!("{mnt_text}/{kpath}.kcl"));
        if system.files.exists(&kcl) {
            be_location(state, fs);
            if let Some(cache) = state.kcl_cache(fs, &kpath) {
                let _ = system.files.copy(&kcl, &cache);
            }
        }
    }
    let _ = system.files.write(&records, &list);
    umount(system.runner, &mnt);

    if !list.is_empty() && select_kernel(system, state, fs).is_some() {
        return Ok(());
    }
    zbm_trace::error!("failed to find kernels on {fs}");
    let _ = system.files.remove_file(&records);
    Err(BootError::NoKernel { fs: fs.to_owned() })
}

/// The kernel to boot from the kernel list of `fs`, as `select_kernel`
/// chooses it: the last one, or the last one `org.zfsbootmenu:kernel` matches.
pub fn select_kernel(system: &System<'_>, state: &State<'_>, fs: &str) -> Option<Kernel> {
    if fs.is_empty() {
        zbm_trace::error!("zfsbe is undefined");
        return None;
    }
    zbm_trace::debug!("zfsbe set to {fs}");
    let list = be_location(state, fs).join("kernels");
    let text = system.files.read_to_string(&list).unwrap_or_default();
    if text.is_empty() {
        zbm_trace::error!("kernel list '{}' missing or empty", list.display());
        return None;
    }
    // tail -1
    let mut chosen = text
        .strip_suffix('\n')
        .unwrap_or(&text)
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .to_owned();

    let property = zfs_value(system.runner, "org.zfsbootmenu:kernel", fs).unwrap_or_default();
    if property != "-" {
        zbm_trace::debug!("org.zfsbootmenu:kernel set to {property}");
        for line in read_lines(&text) {
            let kernel = Kernel::parse(&line);
            if kernels::bash_regex_match(system.runner, &kernel.kernel, &property) == Some(true) {
                zbm_trace::debug!("matched {} to {property}", kernel.kernel);
                chosen = line;
            }
        }
    }
    if chosen.is_empty() {
        zbm_trace::error!("failed to identify kexec arguments for {fs}");
        return None;
    }
    zbm_trace::debug!("using kexec args: {chosen}");
    Some(Kernel::parse(&chosen))
}

/// Unmounts everything below `mountpoint`, deepest first, as
/// `recursive_umount` does when `umount -R` fails.
///
/// # Errors
///
/// [`BootError::Unmount`] with the mount points that stay mounted.
pub fn recursive_umount(system: &System<'_>, mountpoint: &Path) -> Result<(), BootError> {
    let quiet = |command: Command| command.stdout(Stream::Null).stderr(Stream::Null);
    let all = quiet(Command::new("umount").arg("-R").arg(mountpoint));
    if system
        .runner
        .run(&all)
        .is_ok_and(|output| output.status.is_success())
    {
        return Ok(());
    }
    let pattern = format!("^{}", mountpoint.display());
    let table = system
        .files
        .read_to_string(Path::new("/proc/self/mounts"))
        .unwrap_or_default();
    let mut mounts: Vec<(usize, String)> = read_lines(&table)
        .iter()
        .filter_map(|line| line.split_whitespace().nth(1).map(str::to_owned))
        .filter(|mp| kernels::bash_regex_match(system.runner, mp, &pattern) == Some(true))
        .map(|mp| (mp.matches('/').count(), mp))
        .collect();
    // sort -n -k1 -r on "depth,path"
    mounts.sort_by(|a, b| b.cmp(a));
    let mut mounted = Vec::new();
    for (_, mp) in mounts {
        let one = quiet(Command::new("umount").arg(&mp));
        if !system
            .runner
            .run(&one)
            .is_ok_and(|output| output.status.is_success())
        {
            zbm_trace::error!("unable to unmount {mp}");
            mounted.push(mp);
        }
    }
    if mounted.is_empty() {
        Ok(())
    } else {
        Err(BootError::Unmount { mounts: mounted })
    }
}

/// Whether the machine booted with UEFI (`is_efi_system`).
fn is_efi_system(system: &System<'_>) -> bool {
    system.files.is_dir(Path::new("/sys/firmware/efi"))
}

/// Mounts or remounts `efivarfs` read-only (`ro`) or read-write (`rw`), as
/// `mount_efivarfs` does. Does nothing without UEFI.
pub fn mount_efivarfs(system: &System<'_>, mode: &str) {
    let location = "/sys/firmware/efi/efivars";
    if !is_efi_system(system) {
        zbm_trace::debug!("efivarfs unsupported");
        return;
    }
    let check = Command::new("mountpoint")
        .args(["-q", location])
        .stdout(Stream::Null)
        .stderr(Stream::Null);
    let mounted = system
        .runner
        .run(&check)
        .is_ok_and(|output| output.status.is_success());
    let options = if mounted {
        zbm_trace::debug!("remounting '{location}' '{mode}'");
        format!("remount,{mode}")
    } else {
        zbm_trace::debug!("mounting '{location}' '{mode}'");
        mode.to_owned()
    };
    let mount = Command::new("mount")
        .args(["-t", "efivarfs", "efivarfs", location, "-o", &options])
        .stdout(Stream::Inherit);
    let _ = system.runner.run(&mount);
}

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
}

impl Default for BootSettings {
    fn default() -> BootSettings {
        BootSettings {
            set_hostid: true,
            hooks: PathBuf::from(hooks::HOOKS),
            env: Vec::new(),
        }
    }
}

/// The root prefix of `fs` mounted at `mnt`, with the messages of
/// `find_root_prefix`.
fn find_root_prefix(system: &System<'_>, fs: &str, mnt: &Path) -> String {
    zbm_trace::debug!("zfsbe_fs set to {fs}");
    zbm_trace::debug!("zfsbe_mnt set to {}", mnt.display());
    if let Some(prefix) = zfs_value(system.runner, "org.zfsbootmenu:rootprefix", fs)
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
/// pools imported read-write, runs the `teardown.d` hooks and executes the
/// kernel. Returns only if that fails, after telling the user.
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
    let mut hook_env: Vec<(&str, &str)> = settings
        .env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    hook_env.extend([
        ("ZBM_SELECTED_BE", fs.as_str()),
        ("ZBM_SELECTED_KERNEL", kernel.as_str()),
        ("ZBM_SELECTED_INITRAMFS", initramfs.as_str()),
    ]);
    let mut boot_sel_env = hook_env.clone();
    boot_sel_env.push(("ZBM_SELECTED_MOUNTPOINT", &mnt_text));
    hooks::run_stage(
        system,
        &settings.hooks,
        HookStage::BootSel,
        false,
        &boot_sel_env,
    );

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
    let root_prefix = find_root_prefix(system, fs, &mnt);

    let dtb_property = zfs_value(system.runner, "org.zfsbootmenu:devicetree", fs);
    let mut dtb = None;
    match dtb_property.as_deref() {
        Some(property) if !property.is_empty() && property != "-" => {
            zbm_trace::debug!("devicetree property set to '{property}'");
            if let Some(candidate) = commandline::devicetree_candidate(property, kernel, &mnt) {
                if system.files.is_file(&candidate) {
                    zbm_trace::debug!("devicetree file exists: '{}'", candidate.display());
                    dtb = Some(candidate);
                } else {
                    zbm_trace::debug!("devicetree file missing: '{}'", candidate.display());
                }
            }
        }
        _ => {
            zbm_trace::debug!("no devicetree property set");
        }
    }

    let load = KexecLoad {
        mnt: &mnt_text,
        dataset: fs,
        kernel,
        initramfs,
        dtb: dtb.as_deref(),
        root_prefix: &root_prefix,
        args: &cli_args,
    };
    let mut loaded = None;
    let mut output = String::new();
    for _ in 0..2 {
        let (ok, text) = substitution(system.runner, &load.command());
        output = text;
        if ok {
            loaded = Some(());
            break;
        }
    }
    if loaded.is_none() {
        zbm_trace::error!(
            "unable to load {mnt_text}{kernel} and {mnt_text}{initramfs} into memory"
        );
        zbm_trace::error!("{}", output);
        let _ = recursive_umount(system, &mnt);
        TimedPrompt::new()
            .delay(10)
            .message(colorize(
                "red",
                "Unable to load kernel or initramfs into memory",
            ))
            .message(colorize("orange", &format!("{mnt_text}{kernel}")))
            .message(colorize("orange", &format!("{mnt_text}{initramfs}")))
            .run(system);
        return;
    }
    // `if zdebug; then`: the test itself writes an empty line.
    if zbm_trace::enabled(Level::Debug) {
        zbm_trace::debug!("");
        zbm_trace::debug!("loaded {mnt_text}{kernel} and {mnt_text}{initramfs} into memory");
        zbm_trace::debug!("kernel command line: '{root_prefix}{fs} {cli_args}'");
        zbm_trace::debug!("{}", output);
    }

    umount(system.runner, &mnt);

    let pools = Command::new("zpool").args(["list", "-H", "-o", "name"]);
    for pool in here_lines(&substitution(system.runner, &pools).1) {
        if is_writable(system.runner, &pool) {
            zbm_trace::debug!("{pool} is read/write, exporting");
            let _ = export_pool(system.runner, &pool);
        }
    }

    hooks::run_stage(
        system,
        &settings.hooks,
        HookStage::Teardown,
        false,
        &hook_env,
    );

    let booting = format!(
        "\nBooting {} for {} ...\n\n",
        colorize("yellow", kernel),
        colorize("cyan", fs)
    );
    let _ = system.console.write(booting.as_bytes());

    let (_, output) = substitution(system.runner, &commandline::kexec_execute());
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
    use super::*;
    use std::cell::RefCell;
    use zbm_platform::{Installed, KernelLog};

    use serde::{Deserialize, Serialize};
    use zbm_platform::exec::{Output, Status, SystemRunner};
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::legacy::{Bash, log_lines, path};
    use zbm_testkit::parity::Parity;
    use zbm_testkit::{TempDir, parity};

    use crate::commandline::{ETC_HOSTID, SPL_HOSTID};
    use crate::testing::{LINE_LOG_STUBS, Redirect, stub_lines};

    /// A fake for every program the boot runs: it records each call and
    /// answers from `answers`, whose lines are `status<TAB>glob<TAB>output`
    /// (`printf %b` escapes); the first glob that matches the command line
    /// wins, and other commands succeed silently.
    const FAKE: &str = r#"#!/bin/sh
Z="$(cd "$(dirname "$0")/.." && pwd)"
line="$(basename "$0") $*"
printf '%s\n' "$line" >> "$Z/calls"
tab="$(printf '\t')"
while IFS="$tab" read -r status glob output; do
  case "$line" in
    $glob) printf '%b' "$output"; exit "$status" ;;
  esac
done < "$Z/answers"
exit 0
"#;

    /// A hook that records its stage and the variables it gets.
    const HOOK: &str = r#"#!/bin/sh
Z="$(cd "$(dirname "$0")/../.." && pwd)"
printf 'hook %s %s|%s|%s|%s\n' "$(basename "$(dirname "$0")")" "${ZBM_SELECTED_BE}" \
  "${ZBM_SELECTED_KERNEL}" "${ZBM_SELECTED_INITRAMFS}" "${ZBM_SELECTED_MOUNTPOINT}" \
  >> "$Z/calls"
"#;

    const PROGRAMS: [&str; 6] = ["zfs", "zpool", "mount", "umount", "kexec", "mountpoint"];

    /// A machine with one boot environment.
    #[derive(Clone, Debug)]
    struct Machine {
        /// Answers of the fake programs: status, glob, output.
        answers: Vec<(u8, &'static str, &'static str)>,
        /// Files of the boot environment, relative to its root.
        files: Vec<(&'static str, &'static str)>,
        /// Whether hooks are installed.
        hooks: bool,
    }

    const FS: &str = "tank/ROOT/void";

    /// What a run left behind, with the machine's directory written as `$Z`.
    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct Outcome {
        log: String,
        calls: String,
        kernels: Option<String>,
        kcl: Vec<(String, String)>,
        shells: String,
        prompts: usize,
        result: String,
    }

    impl Machine {
        fn dir(&self) -> TempDir {
            let dir = TempDir::new("zbm-boot");
            let answers: String = self
                .answers
                .iter()
                .map(|(status, glob, output)| format!("{status}\t{glob}\t{output}\n"))
                .collect();
            dir.write("answers", answers);
            for program in PROGRAMS {
                dir.write_executable(format!("bin/{program}"), FAKE);
            }
            let mnt = format!("base/environments/{FS}/mnt");
            std::fs::create_dir_all(dir.join(&mnt)).unwrap();
            for (name, contents) in &self.files {
                dir.write(format!("{mnt}/{name}"), contents);
            }
            if self.hooks {
                dir.write_executable("hooks/boot-sel.d/10-record", HOOK);
                dir.write_executable("hooks/teardown.d/10-record", HOOK);
            }
            dir
        }

        fn outcome(
            dir: &TempDir,
            log: &str,
            shells: String,
            prompts: usize,
            result: String,
        ) -> Outcome {
            let z = dir.path().to_string_lossy().into_owned();
            let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
            let kcl_dir = dir.join(format!("base/environments/{FS}/kcl"));
            let mut kcl: Vec<(String, String)> = std::fs::read_dir(&kcl_dir)
                .map(|entries| {
                    entries
                        .map(|entry| {
                            let entry = entry.unwrap();
                            let name = entry.file_name().to_string_lossy().into_owned();
                            (name, std::fs::read_to_string(entry.path()).unwrap())
                        })
                        .collect()
                })
                .unwrap_or_default();
            kcl.sort();
            Outcome {
                log: log.replace(&z, "$Z"),
                calls: read("calls").unwrap_or_default().replace(&z, "$Z"),
                kernels: read(&format!("base/environments/{FS}/kernels")),
                kcl,
                shells: shells.replace(&z, "$Z"),
                prompts,
                result: result.replace(&z, "$Z"),
            }
        }

        /// Runs `call` (shell code using the legacy functions) and returns what
        /// it printed on standard output as the result.
        fn run_legacy(&self, call: &str) -> Outcome {
            let dir = self.dir();
            let z = dir.path().to_string_lossy().into_owned();
            dir.write("stubs.sh", LINE_LOG_STUBS);
            dir.write("empty.conf", "");
            let run_hooks =
                std::fs::read_to_string(path("zfsbootmenu/libexec/zfsbootmenu-run-hooks"))
                    .unwrap()
                    .replace("/lib/profiling-lib.sh", &format!("{z}/stubs.sh"))
                    .replace("/lib/kmsg-log-lib.sh", &format!("{z}/stubs.sh"))
                    .replace("/etc/zfsbootmenu.conf", &format!("{z}/empty.conf"))
                    .replace("/libexec/hooks", &format!("{z}/hooks"));
            dir.write_executable("run-hooks", run_hooks);
            let core =
                std::fs::read_to_string(path("zfsbootmenu/lib/zfsbootmenu-core.sh")).unwrap();
            let function = |name: &str| {
                let start = core.find(&format!("\n{name}() {{\n")).unwrap() + 1;
                let end = start + core[start..].find("\n}\n").unwrap() + 3;
                core[start..end]
                    .replace(SPL_HOSTID, &format!("{z}/spl_hostid"))
                    .replace("/etc/hostid", &format!("{z}/etc_hostid"))
                    .replace("/libexec/zfsbootmenu-run-hooks", &format!("{z}/run-hooks"))
            };
            let output = Bash::new()
                .code(LINE_LOG_STUBS)
                .code(
                    r#"source() {
                        case "$1" in
                          /lib/*) builtin source "$LEGACY/zfsbootmenu/lib/${1#/lib/}" ;;
                          *) builtin source "$@" ;;
                        esac
                      }"#,
                )
                .source("zfsbootmenu/lib/zfsbootmenu-core.sh")
                .code(&function("get_spl_hostid"))
                .code(&function("kexec_kernel"))
                .code(&function("load_key"))
                .code(
                    r#"
                    tput() { :; }
                    timed_prompt() { echo x >> "$Z/prompts"; }
                    emergency_shell() { echo "$1" >> "$Z/shells"; }
                    run() { eval "$1"; }"#,
                )
                .env("Z", &z)
                .env("BASE", &format!("{z}/base"))
                .env(
                    "PATH",
                    &format!("{z}/bin:{}", std::env::var("PATH").unwrap()),
                )
                .env("zbm_set_hostid", "1")
                .env("loglevel", "7")
                .env("ZBM_LOG", &format!("{z}/log"))
                .call("run", &[call], b"");
            let log = log_lines(&std::fs::read_to_string(dir.join("log")).unwrap_or_default());
            let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
            let prompts = read("prompts").lines().count();
            Machine::outcome(&dir, &log, read("shells"), prompts, output.stdout_text())
        }

        /// Runs `call` with the Rust code; it returns the result.
        fn rust<F>(&self, call: F) -> Outcome
        where
            F: FnOnce(&System<'_>, &State<'_>, &mut Shells, &Path) -> String,
        {
            let dir = self.dir();
            let fake = FakeSystem::new();
            let bin = dir.join("bin");
            fake.runner.on(move |command| {
                let program = command.program().to_string_lossy().into_owned();
                let real = |program: PathBuf| {
                    let mut real = Command::new(program)
                        .args(command.get_args())
                        .stdin(command.get_stdin().clone())
                        .stdout(command.get_stdout());
                    for (key, value) in command.get_env() {
                        if let Some(value) = value {
                            real = real.env(key, value);
                        }
                    }
                    real = if command.merges_stderr() {
                        real.merge_stderr()
                    } else {
                        real.stderr(Stream::Null)
                    };
                    Some(SystemRunner.run(&real))
                };
                if PROGRAMS.contains(&program.as_str()) {
                    return real(bin.join(&program));
                }
                if program == "bash" || program.contains("/hooks/") {
                    return real(PathBuf::from(program));
                }
                (program == "tput").then(|| Ok(Output::new(Status::exited(0), "")))
            });
            let files = Redirect::new(&[
                (ETC_HOSTID, &dir.join("etc_hostid")),
                (SPL_HOSTID, &dir.join("spl_hostid")),
            ]);
            let system = System {
                files: &files,
                ..fake.system()
            };
            let state = State::new(&files, dir.join("base"));
            let kmsg = dir.join("kmsg");
            let _log = KernelLog::new(&files, "7", &dir.join("base"), 1)
                .with_device(&kmsg)
                .install();
            let mut shells = Shells::default();
            let result = call(&system, &state, &mut shells, &dir.join("hooks"));
            let log = stub_lines(&std::fs::read_to_string(&kmsg).unwrap_or_default());
            let prompts = fake
                .runner
                .lines()
                .iter()
                .filter(|line| *line == "tput civis")
                .count();
            let shells = shells.messages.borrow().concat();
            Machine::outcome(&dir, &log, shells, prompts, result)
        }
    }

    /// Records the recovery shells the Rust code opens.
    #[derive(Default)]
    struct Shells {
        messages: RefCell<Vec<String>>,
    }

    impl Recovery for Shells {
        fn unlock_late(&mut self) -> Result<(), crate::error::BootError> {
            Err(crate::error::BootError::NoCryptsetup)
        }

        fn emergency_shell(&mut self, message: &str) {
            self.messages.borrow_mut().push(format!("{message}\n"));
        }
    }

    /// The debugging messages of `find_be_initramfs`, which the Rust search
    /// does not repeat.
    fn without_initramfs_search(log: &str) -> String {
        log.lines()
            .filter(|line| {
                ![
                    "DEBUG: kernel path: ",
                    "DEBUG: kernel version: ",
                    "DEBUG: short-matching ",
                    "DEBUG: matching '",
                ]
                .iter()
                .any(|prefix| line.starts_with(prefix))
            })
            .map(|line| format!("{line}\n"))
            .collect()
    }

    fn kernels_machine() -> Machine {
        Machine {
            answers: vec![(0, "zfs get -H -o value org.zfsbootmenu:kernel *", "-\\n")],
            files: vec![
                ("boot/vmlinuz-6.1.10", ""),
                ("boot/initramfs-6.1.10.img", ""),
                ("boot/vmlinuz-6.1.9", ""),
                ("boot/initrd.img-6.1.9", ""),
                ("boot/vmlinuz-6.1.9.kcl", "quiet\n"),
                ("boot/vmlinuz-5.0", ""),
                ("boot/vmlinuz-5.0.kcl", "single\n"),
                ("boot/config-6.1.9", ""),
            ],
            hooks: false,
        }
    }

    #[test]
    fn kernels_are_found_as_the_legacy_function_finds_them() {
        let mut machines = vec![kernels_machine()];
        let mut chosen = kernels_machine();
        chosen.answers = vec![(
            0,
            "zfs get -H -o value org.zfsbootmenu:kernel *",
            "6\\.1\\.9\\n",
        )];
        machines.push(chosen);
        let mut none = kernels_machine();
        none.files = vec![("boot/vmlinuz-1", "")];
        machines.push(none);
        let mut unmountable = kernels_machine();
        unmountable.answers.push((32, "mount *", "mount failed\\n"));
        machines.push(unmountable);

        let mut parity = parity!("kernels_are_found_as_the_legacy_function_finds_them");
        for machine in machines {
            let legacy: Outcome = parity.legacy(format!("{machine:?}"), || {
                machine.run_legacy(&format!("find_be_kernels {FS} && select_kernel {FS}"))
            });
            let rust = machine.rust(|system, state, _, _| {
                if find_kernels(system, state, FS).is_err() {
                    return String::new();
                }
                select_kernel(system, state, FS)
                    .map(|kernel| kernel.line())
                    .unwrap_or_default()
            });
            let legacy = Outcome {
                log: without_initramfs_search(&legacy.log),
                ..legacy
            };
            assert_eq!(rust, legacy);
        }
    }

    fn boot_machine() -> Machine {
        Machine {
            answers: vec![
                (0, "zpool list -H -o name", "tank\\nboot\\n"),
                (0, "zpool get -H -o value readonly tank", "off\\n"),
                (0, "zpool get -H -o value readonly boot", "on\\n"),
                (
                    0,
                    "zfs get -H -o value org.zfsbootmenu:commandline *",
                    "quiet loglevel=4\\n",
                ),
                (
                    0,
                    "zfs get -H -o value org.zfsbootmenu:rootprefix *",
                    "-\\n",
                ),
                (
                    0,
                    "zfs get -H -o value org.zfsbootmenu:devicetree *",
                    "-\\n",
                ),
                (0, "zfs get -H -o value -s inherited *", ""),
                (1, "kexec -e -i", "kexec: no kernel loaded\\n"),
            ],
            files: vec![("etc/os-release", "ID=void\n")],
            hooks: true,
        }
    }

    fn boot(machine: &Machine, parity: &mut Parity) {
        let selected = Kernel {
            dataset: FS.to_owned(),
            kernel: "/boot/vmlinuz-6.1.10".to_owned(),
            initramfs: "/boot/initramfs-6.1.10.img".to_owned(),
        };
        let legacy: Outcome = parity.legacy(format!("{machine:?}"), || {
            machine.run_legacy(&format!(
                "kexec_kernel \"$(printf '%s\\t%s\\t%s' {FS} /boot/vmlinuz-6.1.10 \
                 /boot/initramfs-6.1.10.img)\" >/dev/null"
            ))
        });
        let rust = machine.rust(|system, state, shells, hooks| {
            let settings = BootSettings {
                set_hostid: true,
                hooks: hooks.to_path_buf(),
                env: Vec::new(),
            };
            kexec_kernel(system, state, &settings, &selected, shells);
            String::new()
        });
        assert_eq!(rust, legacy);
    }

    #[test]
    fn boots_as_the_legacy_function_boots() {
        let mut parity = parity!("boots_as_the_legacy_function_boots");
        let machine = boot_machine();
        boot(&machine, &mut parity);

        // A device tree, and a kernel that cannot be loaded.
        let mut machine = boot_machine();
        machine.answers.insert(
            0,
            (
                0,
                "zfs get -H -o value org.zfsbootmenu:devicetree *",
                "board.dtb\\n",
            ),
        );
        machine
            .answers
            .insert(0, (1, "kexec -a -l *", "kexec: bad image\\n"));
        machine.files.push(("boot/dtbs/dtbs-6.1.10/board.dtb", ""));
        boot(&machine, &mut parity);

        // A boot environment that cannot be mounted.
        let mut machine = boot_machine();
        machine
            .answers
            .insert(0, (32, "mount *", "mount: failed\\n"));
        boot(&machine, &mut parity);
    }

    fn fake_log(fake: &FakeSystem) -> Installed {
        fake.files.dir("/zfsbootmenu");
        KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
    }

    #[test]
    fn recursive_unmounts_go_deepest_first() {
        let fake = FakeSystem::new();
        let _log = fake_log(&fake);
        fake.files.file(
            "/proc/self/mounts",
            "tank/a /mnt/be zfs ro 0 0\n\
             tank/a/b /mnt/be/usr zfs ro 0 0\n\
             tank/a/c /mnt/be/usr/local zfs ro 0 0\n\
             tank/d /mnt/other zfs ro 0 0\n\
             tank/e /mnt/bee zfs ro 0 0\n",
        );
        fake.runner.respond("umount -R /mnt/be", 32, "");
        fake.runner.respond("umount /mnt/be/usr", 32, "");
        fake.runner.on(|command| {
            (command.program() == "bash")
                .then(|| Some(SystemRunner.run(command)))
                .flatten()
        });
        let ok = recursive_umount(&fake.system(), Path::new("/mnt/be"));
        assert!(ok.is_err());
        let umounts: Vec<String> = fake
            .runner
            .lines()
            .into_iter()
            .filter(|line| line.starts_with("umount"))
            .collect();
        // `^/mnt/be` is a pattern, so /mnt/bee matches too, as in the legacy code.
        assert_eq!(
            umounts,
            [
                "umount -R /mnt/be",
                "umount /mnt/be/usr/local",
                "umount /mnt/be/usr",
                "umount /mnt/bee",
                "umount /mnt/be",
            ]
        );
        assert!(fake.kmsg().contains("unable to unmount /mnt/be/usr"));
    }

    #[test]
    fn efivarfs_is_mounted_or_remounted() {
        let fake = FakeSystem::new();
        let _log_fake = fake_log(&fake);
        mount_efivarfs(&fake.system(), "ro");
        assert!(fake.runner.lines().is_empty());
        assert!(fake.kmsg().contains("efivarfs unsupported"));

        fake.files.dir("/sys/firmware/efi/efivars");
        fake.runner
            .respond("mountpoint -q /sys/firmware/efi/efivars", 1, "");
        mount_efivarfs(&fake.system(), "ro");
        let fake2 = FakeSystem::new();
        let _log_fake2 = fake_log(&fake2);
        fake2.files.dir("/sys/firmware/efi/efivars");
        mount_efivarfs(&fake2.system(), "rw");
        assert_eq!(
            fake.runner.lines()[1],
            "mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o ro"
        );
        assert_eq!(
            fake2.runner.lines()[1],
            "mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o remount,rw"
        );
    }

    #[test]
    fn writable_pools_and_exports() {
        let fake = FakeSystem::new();
        fake.runner
            .respond("zpool get -H -o value readonly tank", 0, "off\n");
        fake.runner
            .respond("zpool get -H -o value readonly gone", 1, "");
        let _log = fake_log(&fake);
        assert!(is_writable(&fake.runner, "tank"));
        assert!(!is_writable(&fake.runner, "gone"));
        assert!(!is_writable(&fake.runner, ""));
        assert_eq!(export_pool(&fake.runner, "tank"), Ok(()));
        assert!(fake.kmsg().contains("<3>ZFSBootMenu: pool is undefined"));
        assert!(fake.kmsg().contains("tank export process return: 0"));
    }

    #[test]
    fn a_locked_boot_environment_is_not_supported_yet() {
        let machine = Machine {
            answers: vec![
                (0, "zpool list -H -o feature@encryption tank", "active\\n"),
                (0, "zfs get -H -o value encryptionroot *", "tank/ROOT\\n"),
                (0, "zfs get -H -o value keystatus *", "unavailable\\n"),
            ],
            files: vec![],
            hooks: false,
        };
        let outcome = machine.rust(|system, state, shells, hooks| {
            let settings = BootSettings {
                set_hostid: true,
                hooks: hooks.to_path_buf(),
                env: Vec::new(),
            };
            let selected = Kernel::parse("tank/ROOT/void\t/boot/vmlinuz\t/boot/initrd.img");
            kexec_kernel(system, state, &settings, &selected, shells);
            String::new()
        });
        assert_eq!(outcome.shells, format!("{}\n", support::MESSAGE));
        assert!(!outcome.calls.contains("mount"));
        assert!(outcome.log.contains("ERROR: "));
    }
}
