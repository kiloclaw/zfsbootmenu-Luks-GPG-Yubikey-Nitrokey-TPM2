//! The start-up stage `init.d/40-early-hooks`: hooks imported from
//! `zbm.hookroot`, hooks skipped with `zbm.skip_hooks`, and the
//! `early-setup.d` hooks, which run once.

use std::path::Path;

use zbm_boot::hooks::{HOOKS, run_stage};
use zbm_platform::System;
use zbm_platform::console::Screen;
use zbm_platform::files::Files;
use zbm_state::State;
use zbm_tools::mount::Mount;
use zbm_tools::programs;
use zbm_types::HookStage;

use crate::error::StartupError;

/// `zbm.hookroot`: copies the hooks of each stage directory of
/// `<filesystem>//<path>` into `root`. The file
/// system is mounted read-only on `.external_hooks` while they are copied.
/// Failures to copy single hooks are logged, and the others still copied.
///
/// # Errors
///
/// [`StartupError::HookRoot`] when `hook_root` is not `filesystem//path`,
/// [`StartupError::HookMount`] when the file system cannot be mounted.
pub fn import_hooks(
    system: &System<'_>,
    state: &State<'_>,
    root: &Path,
    hook_root: &str,
) -> Result<(), StartupError> {
    let (filesystem, path) = split_hook_root(hook_root)?;
    let mount_point = state.external_hooks();
    let _ = system.files.create_dir_all(&mount_point);
    let Ok(mounted) =
        Mount::new(system.runner, system.files).read_only_shown(filesystem, &mount_point)
    else {
        zbm_trace::error!("failed to mount hook filesystem {filesystem}"; %filesystem);
        return Err(StartupError::HookMount {
            filesystem: filesystem.to_owned(),
        });
    };
    copy_stages(system, &mounted.path().join(path), root);
    drop(mounted);
    Ok(())
}

/// The file system and the path of `zbm.hookroot`, which split at the last
/// `//`; the reason it is malformed is logged.
fn split_hook_root(hook_root: &str) -> Result<(&str, &str), StartupError> {
    let malformed = || StartupError::HookRoot {
        hook_root: hook_root.to_owned(),
    };
    if hook_root.is_empty() {
        zbm_trace::error!("hook root is not defined");
        return Err(malformed());
    }
    match hook_root.rsplit_once("//") {
        Some((filesystem, path)) if !filesystem.is_empty() && !path.is_empty() => {
            Ok((filesystem, path))
        }
        Some((filesystem, _)) if !filesystem.is_empty() => {
            zbm_trace::error!("unable to find hook path: '{hook_root}' is malformed"; %hook_root);
            Err(malformed())
        }
        _ => {
            zbm_trace::error!("unable to find hook device: '{hook_root}' is malformed"; %hook_root);
            Err(malformed())
        }
    }
}

/// Copies the files of each stage directory below `source` into the same
/// directory below `root`; failures are logged, and the other files still
/// copied.
fn copy_stages(system: &System<'_>, source: &Path, root: &Path) {
    for stage in visible_entries(system.files, source) {
        let source_dir = source.join(&stage);
        if !system.files.is_dir(&source_dir) {
            continue;
        }
        let target = root.join(&stage);
        if system.files.create_dir_all(&target).is_err() {
            zbm_trace::warn!(
                "failed to create hook directory {stage}";
                stage = %stage.to_string_lossy()
            );
            continue;
        }
        for name in visible_entries(system.files, &source_dir) {
            let file = source_dir.join(name);
            if !system.files.is_file(&file) {
                continue;
            }
            // `cp` itself, so that the copy gets the mode `cp` gives it.
            if !programs::copy_file(system.runner, &file, &target)
                .is_ok_and(|output| output.status.is_success())
            {
                zbm_trace::warn!("failed to copy user hook {hook}"; hook = %file.display());
            }
        }
    }
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
    zbm_trace::debug!("processing hook skip directives: {skip}"; %skip);
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
                    zbm_trace::info!("Disabling hook: {hook}"; hook = %hook.display());
                    let _ = files.set_mode(&hook, 0);
                }
            }
        }
    }
}

/// The start-up stage `init.d/40-early-hooks`: imports hooks from
/// `zbm.hookroot`, applies `zbm.skip_hooks`, clears the screen and runs the
/// `early-setup.d` hooks once.
///
/// # Errors
///
/// As [`prepare_early_stage`], once the hooks ran.
pub fn early_stage(
    system: &System<'_>,
    state: &State<'_>,
    hook_root: &str,
    skip: Option<&str>,
) -> Result<(), StartupError> {
    early_stage_in(system, state, Path::new(HOOKS), hook_root, skip)
}

/// [`early_stage`] with the hook directories below `root`.
///
/// # Errors
///
/// As [`prepare_early_stage`], once the hooks ran.
pub fn early_stage_in(
    system: &System<'_>,
    state: &State<'_>,
    root: &Path,
    hook_root: &str,
    skip: Option<&str>,
) -> Result<(), StartupError> {
    let prepared = prepare_early_stage(system, state, root, hook_root, skip);
    run_early_stage(system, root);
    prepared
}

/// What comes before the `early-setup.d` hooks below `root` run: the hooks
/// of `zbm.hookroot` are imported, and those of `zbm.skip_hooks` skipped.
///
/// # Errors
///
/// As [`import_hooks`], once `zbm.skip_hooks` is applied: the image's own
/// hooks still run.
pub fn prepare_early_stage(
    system: &System<'_>,
    state: &State<'_>,
    root: &Path,
    hook_root: &str,
    skip: Option<&str>,
) -> Result<(), StartupError> {
    let imported = if hook_root.is_empty() {
        Ok(())
    } else {
        import_hooks(system, state, root, hook_root)
    };
    if let Some(skip) = skip {
        skip_hooks(system.files, root, skip);
    }
    imported
}

/// Runs the `early-setup.d` hooks below `root` once, on a cleared screen.
pub fn run_early_stage(system: &System<'_>, root: &Path) {
    system.console.screen(Screen::Clear);
    run_stage(system, root, HookStage::EarlySetup, true, &[]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::legacy::stub_lines;

    use zbm_platform::{Installed, KernelLog};
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines, path};
    use zbm_testkit::{TempDir, parity};

    fn log(fake: &FakeSystem) -> Installed {
        fake.files.dir("/zfsbootmenu");
        KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
    }

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
            let files = zbm_platform::files::RealFiles;
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
                    legacy.status.is_success(),
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
                    Ok(zbm_platform::exec::Output::new(
                        zbm_platform::exec::Status::exited(1),
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
        assert!(early_stage(&fake.system(), &state, "", Some("20-b")).is_ok());
        assert_eq!(fake.runner.lines(), ["/libexec/hooks/early-setup.d/10-a"]);
        assert_eq!(
            fake.console.screens(),
            [zbm_platform::console::Screen::Clear]
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
    fn a_hook_root_that_cannot_be_read_fails_the_stage_after_the_hooks() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.files
            .file_with_mode("/libexec/hooks/early-setup.d/10-a", "", 0o755);
        let state = State::new(&fake.files, "/zfsbootmenu");
        let error = early_stage(&fake.system(), &state, "no-path", None).unwrap_err();
        assert!(matches!(error, StartupError::HookRoot { .. }), "{error:?}");
        // The image's own hooks ran all the same.
        assert!(
            fake.runner
                .lines()
                .contains(&"/libexec/hooks/early-setup.d/10-a".to_owned())
        );
    }
}
