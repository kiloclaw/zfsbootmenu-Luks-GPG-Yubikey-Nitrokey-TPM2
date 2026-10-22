//! Finding and running hooks: `zfsbootmenu-run-hooks`, `import_zbm_hooks` and
//! the start-up stage `init.d/40-early-hooks`.
//!
//! `zfsbootmenu-run-hooks` runs `"/libexec/hooks/${stage}"/*` in the order the
//! glob gives (by name, in the C locale; names starting with a dot are left
//! out), skipping every entry for which `[ -x ]` is false. For the root user
//! who runs it, `[ -x ]` is true for a file with any execute bit and for every
//! directory.

use std::path::{Path, PathBuf};

use zbm_platform::System;
use zbm_platform::files::{FileKind, Files};
use zbm_tools::programs;
use zbm_types::HookStage;

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
        programs::run_hook(system.runner, hook, env);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::stub_lines;
    use zbm_platform::exec::{Input, Stream};
    use zbm_platform::{Installed, KernelLog};
    use zbm_testkit::fakes::{FakeFiles, FakeSystem, FileOperation};
    use zbm_testkit::legacy::Bash;
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
                .then(|| Err(std::io::Error::from_raw_os_error(8)))
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
        use zbm_platform::files::RealFiles;

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
