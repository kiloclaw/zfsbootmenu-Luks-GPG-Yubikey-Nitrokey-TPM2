//! Finding and running hooks: `zfsbootmenu-run-hooks`, `import_zbm_hooks` and
//! the start-up stage `init.d/40-early-hooks`.
//!
//! `zfsbootmenu-run-hooks` runs `"/libexec/hooks/${stage}"/*` in the order the
//! glob gives (by name, in the C locale; names starting with a dot are left
//! out), skipping every entry for which `[ -x ]` is false. For the root user
//! who runs it, `[ -x ]` is true for a file with any execute bit and for every
//! directory.

use std::path::{Path, PathBuf};

use zbm_core::exec::{Command, Input, Stream};
use zbm_state::State;
use zbm_system::files::{FileKind, Files};
use zbm_system::{Runner, System};
use zbm_types::HookStage;

use crate::error::BootError;

/// `ENOEXEC`: a file the kernel cannot run, such as a script without `#!`.
const ENOEXEC: i32 = 8;

/// Where the hook directories are in the image.
pub const HOOKS: &str = "/libexec/hooks";

/// The hooks of `stage` below `root` that would run, in order. A missing
/// directory has none.
pub fn executable_hooks(files: &dyn Files, root: &Path, stage: HookStage) -> Vec<PathBuf> {
    let dir = root.join(stage.as_str());
    let Ok(names) = files.read_dir(&dir) else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|name| !name.as_encoded_bytes().starts_with(b"."))
        .map(|name| dir.join(name))
        .filter(|path| {
            files.metadata(path).is_ok_and(|metadata| {
                metadata.kind == FileKind::Directory || metadata.is_executable()
            })
        })
        .collect()
}

/// Runs the hooks of `stage` below `root`, as `zfsbootmenu-run-hooks` does,
/// with the standard streams of this process and its environment plus `env`.
/// With `once`, each hook then loses its permissions and gets the suffix
/// `.completed`. Returns how many hooks ran; a missing directory runs none.
pub fn run_stage(
    system: &System<'_>,
    root: &Path,
    stage: HookStage,
    once: bool,
    env: &[(&str, &str)],
) -> usize {
    if !system.files.is_dir(&root.join(stage.as_str())) {
        zbm_trace::debug!("no hook directory for {stage}");
        return 0;
    }
    let hooks = executable_hooks(system.files, root, stage);
    for hook in &hooks {
        zbm_trace::info!("processing {}", hook.display());
        run_hook(system.runner, hook, env);
        if once {
            zbm_trace::info!("Disabling hook after execution: {}", hook.display());
            let _ = system.files.set_mode(hook, 0);
            let mut completed = hook.clone().into_os_string();
            completed.push(".completed");
            let _ = system.files.rename(hook, Path::new(&completed));
        }
    }
    hooks.len()
}

/// Runs one hook as Bash runs `"${_hook}"`: its exit status is ignored, and a
/// script the kernel cannot run (no `#!` line) runs with Bash.
fn run_hook(runner: &dyn Runner, hook: &Path, env: &[(&str, &str)]) {
    let with_env = |command: Command| {
        env.iter()
            .fold(command, |command, (key, value)| command.env(key, value))
            .stdin(Input::Inherit)
            .stdout(Stream::Inherit)
            .stderr(Stream::Inherit)
    };
    let direct = with_env(Command::new(hook));
    if let Err(error) = runner.run(&direct)
        && error.raw_os_error() == Some(ENOEXEC)
    {
        let _ = runner.run(&with_env(Command::new("bash").arg(hook)));
    }
}

/// `zbm.hookroot`: copies the hooks of each stage directory of
/// `<filesystem>//<path>` into `root`, as `import_zbm_hooks` does. The file
/// system is mounted read-only on `.external_hooks` while they are copied.
/// Failures to copy single hooks are logged, and the others still copied.
///
/// # Errors
///
/// [`BootError::HookRoot`] when `hook_root` is not `filesystem//path`,
/// [`BootError::HookMount`] when the file system cannot be mounted.
pub fn import_hooks(
    system: &System<'_>,
    state: &State<'_>,
    root: &Path,
    hook_root: &str,
) -> Result<(), BootError> {
    let malformed = || BootError::HookRoot {
        hook_root: hook_root.to_owned(),
    };
    if hook_root.is_empty() {
        zbm_trace::error!("hook root is not defined");
        return Err(malformed());
    }
    // `${hook_root%//*}` and `${hook_root##*//}` split at the last `//`.
    let Some((filesystem, path)) = hook_root.rsplit_once("//") else {
        zbm_trace::error!("unable to find hook device: '{hook_root}' is malformed");
        return Err(malformed());
    };
    if filesystem.is_empty() {
        zbm_trace::error!("unable to find hook device: '{hook_root}' is malformed");
        return Err(malformed());
    }
    if path.is_empty() {
        zbm_trace::error!("unable to find hook path: '{hook_root}' is malformed");
        return Err(malformed());
    }

    let mount_point = state.external_hooks();
    let _ = system.files.create_dir_all(&mount_point);
    let mount = Command::new("mount")
        .arg("-r")
        .arg(filesystem)
        .arg(&mount_point)
        .stdout(Stream::Inherit);
    if !system
        .runner
        .run(&mount)
        .is_ok_and(|output| output.status.success())
    {
        zbm_trace::error!("failed to mount hook filesystem {filesystem}");
        return Err(BootError::HookMount {
            filesystem: filesystem.to_owned(),
        });
    }

    let source = mount_point.join(path);
    for stage in visible_entries(system.files, &source) {
        let source_dir = source.join(&stage);
        if !system.files.is_dir(&source_dir) {
            continue;
        }
        let target = root.join(&stage);
        if system.files.create_dir_all(&target).is_err() {
            zbm_trace::warn!(
                "failed to create hook directory {}",
                stage.to_string_lossy()
            );
            continue;
        }
        for name in visible_entries(system.files, &source_dir) {
            let file = source_dir.join(name);
            if !system.files.is_file(&file) {
                continue;
            }
            // `cp` itself, so that the copy gets the mode `cp` gives it.
            let copy = Command::new("cp")
                .arg(&file)
                .arg(&target)
                .stdout(Stream::Null)
                .stderr(Stream::Null);
            if !system
                .runner
                .run(&copy)
                .is_ok_and(|output| output.status.success())
            {
                zbm_trace::warn!("failed to copy user hook {}", file.display());
            }
        }
    }

    let umount = Command::new("umount")
        .arg(&mount_point)
        .stdout(Stream::Inherit);
    let _ = system.runner.run(&umount);
    Ok(())
}

/// The names a shell glob `dir/*` gives: sorted, without names starting with a
/// dot.
fn visible_entries(files: &dyn Files, dir: &Path) -> Vec<std::ffi::OsString> {
    files
        .read_dir(dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| !name.as_encoded_bytes().starts_with(b"."))
        .collect()
}

/// `zbm.skip_hooks`: removes the permissions of every hook, in any stage
/// directory below `root`, whose name is in the comma-separated `skip` list.
pub fn skip_hooks(files: &dyn Files, root: &Path, skip: &str) {
    if skip.is_empty() {
        return;
    }
    zbm_trace::debug!("processing hook skip directives: {skip}");
    let stages: Vec<_> = visible_entries(files, root)
        .into_iter()
        .filter(|name| name.as_encoded_bytes().ends_with(b".d"))
        .collect();
    for name in skip.split(',').filter(|name| !name.is_empty()) {
        for stage in &stages {
            for hook in visible_entries(files, &root.join(stage)) {
                if hook.as_encoded_bytes() != name.as_bytes() {
                    continue;
                }
                let hook = root.join(stage).join(hook);
                if files.exists(&hook) {
                    zbm_trace::info!("Disabling hook: {}", hook.display());
                    let _ = files.set_mode(&hook, 0);
                }
            }
        }
    }
}

/// The start-up stage `init.d/40-early-hooks`: imports hooks from
/// `zbm.hookroot`, applies `zbm.skip_hooks`, clears the screen and runs the
/// `early-setup.d` hooks once.
pub fn early_stage(system: &System<'_>, state: &State<'_>, hook_root: &str, skip: Option<&str>) {
    early_stage_in(system, state, Path::new(HOOKS), hook_root, skip);
}

/// [`early_stage`] with the hook directories below `root`.
pub fn early_stage_in(
    system: &System<'_>,
    state: &State<'_>,
    root: &Path,
    hook_root: &str,
    skip: Option<&str>,
) {
    if !hook_root.is_empty() {
        let _ = import_hooks(system, state, root, hook_root);
    }
    if let Some(skip) = skip {
        skip_hooks(system.files, root, skip);
    }
    crate::prompt::tput(system, &["clear"]);
    run_stage(system, root, HookStage::EarlySetup, true, &[]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::stub_lines;
    use zbm_system::{Installed, KernelLog};
    use zbm_testkit::fakes::{FakeFiles, FakeSystem, FileOperation};
    use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines, path};
    use zbm_testkit::{TempDir, parity};

    fn log(fake: &FakeSystem) -> Installed {
        fake.files.dir("/zfsbootmenu");
        KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
    }

    #[test]
    fn runs_each_hook_with_the_stage_environment() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.files
            .file_with_mode("/libexec/hooks/boot-sel.d/20-b", "", 0o755)
            .file_with_mode("/libexec/hooks/boot-sel.d/10-a", "", 0o755)
            .file_with_mode("/libexec/hooks/boot-sel.d/30-off", "", 0o644);
        let env = [("ZBM_SELECTED_BE", "tank/ROOT/void")];
        let ran = run_stage(
            &fake.system(),
            Path::new(HOOKS),
            HookStage::BootSel,
            false,
            &env,
        );
        assert!(ran > 0);
        assert_eq!(
            fake.runner.lines(),
            [
                "/libexec/hooks/boot-sel.d/10-a",
                "/libexec/hooks/boot-sel.d/20-b"
            ]
        );
        for call in fake.runner.calls() {
            assert_eq!(call.get_stdin(), &Input::Inherit);
            assert_eq!(call.get_stdout(), Stream::Inherit);
            assert_eq!(call.get_stderr(), Stream::Inherit);
            assert_eq!(
                call.get_env(),
                [("ZBM_SELECTED_BE".into(), Some("tank/ROOT/void".into()))]
            );
        }
        assert_eq!(
            stub_lines(&fake.kmsg()),
            "INFO: processing /libexec/hooks/boot-sel.d/10-a\n\
             INFO: processing /libexec/hooks/boot-sel.d/20-b\n"
        );
        // Without a directory, or without executable hooks, none runs.
        assert_eq!(
            run_stage(
                &fake.system(),
                Path::new(HOOKS),
                HookStage::LoadKey,
                false,
                &[],
            ),
            0
        );
        assert!(fake.kmsg().contains("no hook directory for load-key.d"));
        fake.files.dir("/libexec/hooks/setup.d");
        assert_eq!(
            run_stage(
                &fake.system(),
                Path::new(HOOKS),
                HookStage::Setup,
                false,
                &[],
            ),
            0
        );
    }

    #[test]
    fn early_hooks_run_once() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.files
            .file_with_mode("/libexec/hooks/early-setup.d/10-a", "", 0o755)
            .file_with_mode("/libexec/hooks/early-setup.d/20-b", "", 0o700);
        let run = || {
            run_stage(
                &fake.system(),
                Path::new(HOOKS),
                HookStage::EarlySetup,
                true,
                &[],
            )
        };
        assert!(run() > 0);
        let operations: Vec<FileOperation> = fake
            .files
            .operations()
            .into_iter()
            .filter(|operation| {
                matches!(
                    operation,
                    FileOperation::SetMode(..) | FileOperation::Rename(..)
                )
            })
            .collect();
        let hook = |name: &str| PathBuf::from(format!("/libexec/hooks/early-setup.d/{name}"));
        assert_eq!(
            operations,
            [
                FileOperation::SetMode(hook("10-a"), 0),
                FileOperation::Rename(hook("10-a"), hook("10-a.completed")),
                FileOperation::SetMode(hook("20-b"), 0),
                FileOperation::Rename(hook("20-b"), hook("20-b.completed")),
            ]
        );
        assert!(fake.kmsg().contains(
            "<6>ZFSBootMenu: Disabling hook after execution: /libexec/hooks/early-setup.d/20-b"
        ));
        // The second time, nothing is left to run.
        assert_eq!(run(), 0);
        assert_eq!(fake.runner.lines().len(), 2);
    }

    #[test]
    fn scripts_without_an_interpreter_line_run_with_bash() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.files
            .file_with_mode("/libexec/hooks/setup.d/10-plain", "", 0o755);
        fake.runner.on(|command| {
            (command.program() == "/libexec/hooks/setup.d/10-plain")
                .then(|| Err(std::io::Error::from_raw_os_error(ENOEXEC)))
        });
        let env = [("BOOTFS", "tank/ROOT/void")];
        assert!(
            run_stage(
                &fake.system(),
                Path::new(HOOKS),
                HookStage::Setup,
                false,
                &env,
            ) > 0
        );
        assert_eq!(
            fake.runner.lines(),
            [
                "/libexec/hooks/setup.d/10-plain",
                "bash /libexec/hooks/setup.d/10-plain"
            ]
        );
        assert_eq!(fake.runner.calls()[1].get_env().len(), 1);
    }

    /// A hook root with executable hooks in several stages.
    fn skip_root() -> TempDir {
        let dir = TempDir::new("zbm-skip");
        for name in [
            "early-setup.d/10-a",
            "early-setup.d/20-b.sh",
            "setup.d/10-a",
            "setup.d/.10-a",
            "teardown.d/30-c",
            "other/10-a",
        ] {
            let file = dir.write(name, "");
            let mode = std::os::unix::fs::PermissionsExt::from_mode(0o755);
            std::fs::set_permissions(&file, mode).unwrap();
        }
        dir
    }

    /// The modes of the hooks under `dir`, by path relative to it.
    fn skip_modes(dir: &TempDir) -> Vec<(String, u32)> {
        let mut modes = Vec::new();
        for stage in ["early-setup.d", "setup.d", "teardown.d", "other"] {
            for entry in std::fs::read_dir(dir.join(stage)).unwrap() {
                let entry = entry.unwrap();
                let mode = entry.metadata().unwrap().permissions();
                let mode = std::os::unix::fs::PermissionsExt::mode(&mode) & 0o7777;
                let name = entry.file_name().to_string_lossy().into_owned();
                modes.push((format!("{stage}/{name}"), mode));
            }
        }
        modes.sort();
        modes
    }

    /// `text` with the directory written as `$DIR`.
    fn without_dir(text: &str, dir: &TempDir) -> String {
        text.replace(&*dir.path().to_string_lossy(), "$DIR")
    }

    #[test]
    fn skipped_hooks_match_the_legacy_loop() {
        let mut parity = parity!("skipped_hooks_match_the_legacy_loop");
        for skip in [
            "",
            "10-a",
            "10-a,,20-b.sh",
            ",missing,30-c",
            "early-setup.d",
        ] {
            let (legacy_modes, legacy_log): (Vec<(String, u32)>, String) =
                parity.legacy(skip, || {
                    let early =
                        std::fs::read_to_string(path("zfsbootmenu/init.d/40-early-hooks")).unwrap();
                    let start = early.find("# Remove the executable bit").unwrap();
                    let end = early.find("# Run early setup hooks").unwrap();
                    let dir = skip_root();
                    let root = dir.path().to_string_lossy().into_owned();
                    let snippet = early[start..end].replace("/libexec/hooks", &root);
                    let legacy = Bash::new()
                        .code(LOG_STUBS)
                        .code(&format!(
                            "get_zbm_arg() {{ echo \"$SKIP\"; }}\nrun() {{\n{snippet}\n}}"
                        ))
                        .env("SKIP", skip)
                        .call("run", &[], b"");
                    (
                        skip_modes(&dir),
                        without_dir(&log_lines(&legacy.stderr_text()), &dir),
                    )
                });

            let dir = skip_root();
            let files = zbm_system::files::RealFiles;
            let kmsg = dir.join("kmsg");
            let _log = KernelLog::new(&files, "7", dir.path(), 1)
                .with_device(&kmsg)
                .install();
            skip_hooks(&files, dir.path(), skip);
            assert_eq!(skip_modes(&dir), legacy_modes, "{skip}");
            let rust_log = stub_lines(&std::fs::read_to_string(&kmsg).unwrap_or_default());
            assert_eq!(without_dir(&rust_log, &dir), legacy_log, "{skip}");
        }
    }

    #[test]
    fn malformed_hook_roots_match_the_legacy_checks() {
        let mut parity = parity!("malformed_hook_roots_match_the_legacy_checks");
        for hook_root in ["", "tank", "tank//", "//hooks", "a//b//", "tank/hooks//zbm"] {
            let (success, logged): (bool, String) = parity.legacy(hook_root, || {
                let core =
                    std::fs::read_to_string(path("zfsbootmenu/lib/zfsbootmenu-core.sh")).unwrap();
                let start = core.find("\nimport_zbm_hooks() {\n").unwrap() + 1;
                let end = start + core[start..].find("\n}\n").unwrap() + 3;
                let dir = TempDir::new("zbm-hookroot");
                let legacy = Bash::new()
                    .code(LOG_STUBS)
                    .code(&core[start..end])
                    .code("mount() { return 1; }")
                    .env("BASE", &dir.path().to_string_lossy())
                    .call("import_zbm_hooks", &[hook_root], b"");
                (
                    legacy.status.success(),
                    log_lines(&legacy.stderr_text())
                        .replace(&*dir.path().to_string_lossy(), "/zfsbootmenu"),
                )
            });

            let fake = FakeSystem::new();

            let _log = log(&fake);
            fake.runner.respond_program("mount", 1, "");
            let state = State::new(&fake.files, "/zfsbootmenu");
            let imported = import_hooks(&fake.system(), &state, Path::new(HOOKS), hook_root);
            assert_eq!(imported.is_ok(), success, "{hook_root}");
            assert_eq!(stub_lines(&fake.kmsg()), logged, "{hook_root}");
        }
    }

    #[test]
    fn imports_the_stage_directories_of_a_hook_root() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        let mnt = "/zfsbootmenu/.external_hooks";
        fake.files
            .file(format!("{mnt}/zbm/early-setup.d/10-net"), "")
            .file(format!("{mnt}/zbm/early-setup.d/.hidden"), "")
            .dir(format!("{mnt}/zbm/early-setup.d/sub"))
            .file(format!("{mnt}/zbm/teardown.d/90-off"), "")
            .file(format!("{mnt}/zbm/README"), "");
        fake.runner.on(|command| {
            command
                .display()
                .ends_with("90-off /libexec/hooks/teardown.d")
                .then(|| {
                    Ok(zbm_core::exec::Output::new(
                        zbm_core::exec::Status::exited(1),
                        "",
                    ))
                })
        });
        let state = State::new(&fake.files, "/zfsbootmenu");
        assert!(import_hooks(&fake.system(), &state, Path::new(HOOKS), "tank/hooks//zbm",).is_ok());
        assert_eq!(
            fake.runner.lines(),
            [
                format!("mount -r tank/hooks {mnt}"),
                format!("cp {mnt}/zbm/early-setup.d/10-net /libexec/hooks/early-setup.d"),
                format!("cp {mnt}/zbm/teardown.d/90-off /libexec/hooks/teardown.d"),
                format!("umount {mnt}"),
            ]
        );
        assert!(fake.files.is_dir(Path::new("/libexec/hooks/teardown.d")));
        assert!(fake.kmsg().contains(&format!(
            "<4>ZFSBootMenu: failed to copy user hook {mnt}/zbm/teardown.d/90-off"
        )));

        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.runner.respond_program("mount", 32, "");
        let state = State::new(&fake.files, "/zfsbootmenu");
        assert!(
            import_hooks(&fake.system(), &state, Path::new(HOOKS), "tank/hooks//zbm",).is_err()
        );
        assert!(
            fake.kmsg()
                .contains("failed to mount hook filesystem tank/hooks")
        );
        assert_eq!(fake.runner.lines().len(), 1);
    }

    #[test]
    fn the_early_stage_imports_skips_and_runs_once() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.files
            .file_with_mode("/libexec/hooks/early-setup.d/10-a", "", 0o755)
            .file_with_mode("/libexec/hooks/early-setup.d/20-b", "", 0o755);
        let state = State::new(&fake.files, "/zfsbootmenu");
        early_stage(&fake.system(), &state, "", Some("20-b"));
        assert_eq!(
            fake.runner.lines(),
            ["tput clear", "/libexec/hooks/early-setup.d/10-a"]
        );
        assert!(
            fake.files
                .exists(Path::new("/libexec/hooks/early-setup.d/10-a.completed"))
        );
        assert!(
            fake.kmsg()
                .contains("Disabling hook: /libexec/hooks/early-setup.d/20-b")
        );
    }

    #[test]
    fn lists_what_the_legacy_glob_and_test_run() {
        let files = FakeFiles::new();
        let root = Path::new(HOOKS);
        files.file_with_mode("/libexec/hooks/early-setup.d/20-b.sh", "", 0o755);
        files.file_with_mode("/libexec/hooks/early-setup.d/10-a.sh", "", 0o700);
        files.file_with_mode("/libexec/hooks/early-setup.d/30-off.sh", "", 0o644);
        files.file_with_mode("/libexec/hooks/early-setup.d/40-done.sh.completed", "", 0);
        files.file_with_mode("/libexec/hooks/early-setup.d/.hidden", "", 0o755);
        files.file_with_mode("/libexec/hooks/early-setup.d/Z-upper", "", 0o001);
        files.dir("/libexec/hooks/early-setup.d/50-dir");
        files.symlink("/libexec/hooks/early-setup.d/60-link", "20-b.sh");
        files.symlink("/libexec/hooks/early-setup.d/70-dangling", "missing");
        let names: Vec<String> = executable_hooks(&files, root, HookStage::EarlySetup)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["10-a.sh", "20-b.sh", "50-dir", "60-link", "Z-upper"]
        );
        assert!(executable_hooks(&files, root, HookStage::LoadKey).is_empty());
    }

    /// Whether the tests run as root, as the legacy code always does; `[ -x ]`
    /// means something else for other users.
    fn running_as_root() -> bool {
        std::fs::read_to_string("/proc/self/status").is_ok_and(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .and_then(|ids| ids.split_whitespace().nth(1))
                == Some("0")
        })
    }

    #[test]
    fn matches_the_legacy_glob_on_real_files() {
        use std::os::unix::fs::PermissionsExt;
        use zbm_system::files::RealFiles;

        if !running_as_root() {
            eprintln!("skipped: the legacy code runs as root");
            return;
        }
        let stage_dir = || {
            let dir = TempDir::new("zbm-hooks");
            let stage = dir.join("teardown.d");
            std::fs::create_dir(&stage).unwrap();
            for (name, mode) in [
                ("90-xhci", 0o755),
                ("80-luks", 0o700),
                ("85-off", 0o644),
                ("10 space", 0o100),
                (".hidden", 0o755),
                ("x.completed", 0o000),
                ("B", 0o010),
            ] {
                let path = dir.write(format!("teardown.d/{name}"), "");
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            }
            std::fs::create_dir(stage.join("sub")).unwrap();
            std::os::unix::fs::symlink("missing", stage.join("dangling")).unwrap();
            dir
        };
        let mut parity = parity!("matches_the_legacy_glob_on_real_files");
        let legacy: Vec<String> = parity.legacy("teardown.d", || {
            let dir = stage_dir();
            // The loop of zfsbootmenu-run-hooks
            let output = Bash::new()
                .code(
                    r#"list_hooks() {
                      for _hook in "$1"/*; do
                        [ -x "${_hook}" ] || continue; echo "${_hook##*/}"
                      done
                    }"#,
                )
                .call(
                    "list_hooks",
                    &[&dir.join("teardown.d").to_string_lossy()],
                    b"",
                );
            output.stdout_text().lines().map(str::to_owned).collect()
        });
        let dir = stage_dir();
        let rust: Vec<String> = executable_hooks(&RealFiles, dir.path(), HookStage::Teardown)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(rust, legacy);
        assert_eq!(rust, ["10 space", "80-luks", "90-xhci", "B", "sub"]);
    }
}
