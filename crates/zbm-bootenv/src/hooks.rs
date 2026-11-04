//! Finding and running the hooks of a stage, for the boot and for the
//! early-setup hooks of the start-up stage `init.d/40-early-hooks`.
//!
//! `zfsbootmenu-run-hooks` runs `"/libexec/hooks/${stage}"/*` in the order the
//! glob gives (by name, in the C locale; names starting with a dot are left
//! out), skipping every entry for which `[ -x ]` is false. For the root user
//! who runs it, `[ -x ]` is true for a file with any execute bit and for every
//! directory.
//!
//! Each stage's hooks get one environment, which [`HookEnv::vars`] builds
//! from what the stage knows (ADR 0017, point 7): the variables of every
//! hook of the boot ([`Variables`]: those of `/etc/zfsbootmenu.conf`, and
//! `BOOTFS` once it is known), then the stage's own.

use std::path::{Path, PathBuf};

use zbm_platform::Context;
use zbm_platform::files::{FileKind, Files};
use zbm_tools::programs;
use zbm_types::{DatasetName, HookStage};

use crate::kernels::Kernel;

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

/// The variables every hook of a boot gets, whichever process runs it: those
/// of `/etc/zfsbootmenu.conf`, as the legacy runtime's hooks got them from
/// the script that sourced it, and `BOOTFS` once it is known.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Variables(Vec<(String, String)>);

impl From<Vec<(String, String)>> for Variables {
    fn from(variables: Vec<(String, String)>) -> Variables {
        Variables(variables)
    }
}

impl Extend<(String, String)> for Variables {
    fn extend<T: IntoIterator<Item = (String, String)>>(&mut self, variables: T) {
        self.0.extend(variables);
    }
}

impl Variables {
    /// These variables with `BOOTFS` set to `bootfs`, the default boot
    /// environment now, as the legacy menu set it when the default changed.
    #[must_use]
    pub fn with_bootfs(mut self, bootfs: &DatasetName) -> Variables {
        self.0.retain(|(name, _)| name != "BOOTFS");
        self.0.push(("BOOTFS".to_owned(), bootfs.to_string()));
        self
    }

    /// Each variable, as its name and its value.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

/// A stage of hooks and what it knows of the boot.
#[derive(Clone, Copy, Debug)]
pub enum StageEnv<'a> {
    /// `early-setup.d`, while start-up initializes.
    EarlySetup,
    /// `setup.d`, before the menu.
    Setup,
    /// `load-key.d`, for the locked `fs` whose encryption root is `root`.
    LoadKey {
        /// The locked filesystem.
        fs: &'a str,
        /// Its encryption root.
        root: &'a str,
    },
    /// `boot-sel.d`, once `selected` is mounted at `mountpoint`.
    BootSel {
        /// The boot environment and the kernel chosen.
        selected: &'a Kernel,
        /// Where the boot environment is mounted.
        mountpoint: &'a str,
    },
    /// `teardown.d`, before `selected` is executed.
    Teardown {
        /// The boot environment and the kernel chosen.
        selected: &'a Kernel,
    },
}

impl StageEnv<'_> {
    /// The stage's hook directory.
    pub fn stage(&self) -> HookStage {
        match self {
            StageEnv::EarlySetup => HookStage::EarlySetup,
            StageEnv::Setup => HookStage::Setup,
            StageEnv::LoadKey { .. } => HookStage::LoadKey,
            StageEnv::BootSel { .. } => HookStage::BootSel,
            StageEnv::Teardown { .. } => HookStage::Teardown,
        }
    }
}

/// The environment of a stage's hooks: the boot's [`Variables`] and what the
/// stage knows.
#[derive(Clone, Copy, Debug)]
pub struct HookEnv<'a> {
    /// The variables every hook gets.
    pub variables: &'a Variables,
    /// The stage, and what it knows.
    pub stage: StageEnv<'a>,
}

impl<'a> HookEnv<'a> {
    /// The variables the stage's hooks get beside the environment of this
    /// process, as the legacy runtime gave them: the boot's, then
    /// `ZFSBOOTMENU_INITIALIZATION=yes` in `early-setup.d`, `ZBM_LOCKED_FS`
    /// and `ZBM_ENCRYPTION_ROOT` in `load-key.d`, and `ZBM_SELECTED_BE`,
    /// `ZBM_SELECTED_KERNEL` and `ZBM_SELECTED_INITRAMFS` in `boot-sel.d`,
    /// with `ZBM_SELECTED_MOUNTPOINT`, and in `teardown.d`.
    pub fn vars(&self) -> Vec<(&'a str, &'a str)> {
        let mut vars: Vec<(&str, &str)> = self.variables.iter().collect();
        let selected = |kernel: &'a Kernel| {
            [
                ("ZBM_SELECTED_BE", kernel.dataset.as_str()),
                ("ZBM_SELECTED_KERNEL", kernel.kernel.as_str()),
                ("ZBM_SELECTED_INITRAMFS", kernel.initramfs.as_str()),
            ]
        };
        match self.stage {
            StageEnv::EarlySetup => vars.push(("ZFSBOOTMENU_INITIALIZATION", "yes")),
            StageEnv::Setup => {}
            StageEnv::LoadKey { fs, root } => {
                vars.extend([("ZBM_LOCKED_FS", fs), ("ZBM_ENCRYPTION_ROOT", root)]);
            }
            StageEnv::BootSel {
                selected: kernel,
                mountpoint,
            } => {
                vars.extend(selected(kernel));
                vars.push(("ZBM_SELECTED_MOUNTPOINT", mountpoint));
            }
            StageEnv::Teardown { selected: kernel } => vars.extend(selected(kernel)),
        }
        vars
    }
}

/// How many bytes of a hook tell how it runs.
const START: usize = 256;

/// Runs the hooks of `env`'s stage below `root`, as `zfsbootmenu-run-hooks`
/// does, with the standard streams of this process and its environment plus
/// `env`'s variables. With `once`, each hook then loses its permissions and
/// gets the suffix `.completed`. Returns how many hooks ran; a missing
/// directory runs none.
pub fn run_stage(context: &Context<'_>, root: &Path, env: &HookEnv<'_>, once: bool) -> usize {
    let stage = env.stage.stage();
    if !context.files().is_dir(&root.join(stage.as_str())) {
        zbm_trace::debug!("no hook directory for {stage}"; %stage);
        return 0;
    }
    let vars = env.vars();
    let hooks = executable_hooks(context.files(), root, stage);
    for hook in &hooks {
        zbm_trace::info!("processing {hook}"; hook = %hook.display());
        let start = context.files().read_prefix(hook, START).ok();
        programs::run_hook(context.runner(), hook, start.as_deref(), &vars);
        if once {
            zbm_trace::info!("Disabling hook after execution: {hook}"; hook = %hook.display());
            let _ = context.files().set_mode(hook, 0);
            let mut completed = hook.clone().into_os_string();
            completed.push(".completed");
            let _ = context.files().rename(hook, Path::new(&completed));
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

    /// The environment of a stage without the boot's variables.
    fn bare(stage: StageEnv<'_>) -> HookEnv<'_> {
        static NONE: Variables = Variables(Vec::new());
        HookEnv {
            variables: &NONE,
            stage,
        }
    }

    #[test]
    fn runs_each_hook_with_the_stage_environment() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        let script = "#!/bin/sh\n";
        fake.files
            .file_with_mode("/libexec/hooks/setup.d/20-b", script, 0o755)
            .file_with_mode("/libexec/hooks/setup.d/10-a", script, 0o755)
            .file_with_mode("/libexec/hooks/setup.d/30-off", script, 0o644);
        let variables = Variables::from(vec![("BOOTFS".to_owned(), "tank/ROOT/void".to_owned())]);
        let env = HookEnv {
            variables: &variables,
            stage: StageEnv::Setup,
        };
        let ran = run_stage(&fake.context(), Path::new(HOOKS), &env, false);
        assert!(ran > 0);
        assert_eq!(
            fake.runner.lines(),
            ["/libexec/hooks/setup.d/10-a", "/libexec/hooks/setup.d/20-b"]
        );
        for call in fake.runner.calls() {
            assert_eq!(call.get_stdin(), &Input::Inherit);
            assert_eq!(call.get_stdout(), Stream::Inherit);
            assert_eq!(call.get_stderr(), Stream::Inherit);
            assert_eq!(
                call.get_env(),
                [("BOOTFS".into(), Some("tank/ROOT/void".into()))]
            );
        }
        assert_eq!(
            stub_lines(&fake.kmsg()),
            "INFO: processing /libexec/hooks/setup.d/10-a\n\
             INFO: processing /libexec/hooks/setup.d/20-b\n"
        );
        // Without a directory, or without executable hooks, none runs.
        let locked = bare(StageEnv::LoadKey {
            fs: "tank/ROOT/void",
            root: "tank",
        });
        assert_eq!(
            run_stage(&fake.context(), Path::new(HOOKS), &locked, false),
            0
        );
        assert!(fake.kmsg().contains("no hook directory for load-key.d"));
        fake.files.dir("/libexec/hooks/early-setup.d");
        assert_eq!(
            run_stage(
                &fake.context(),
                Path::new(HOOKS),
                &bare(StageEnv::EarlySetup),
                false
            ),
            0
        );
    }

    /// ADR 0017, point 7: each stage's variables, after the boot's, as the
    /// legacy runtime gave them.
    #[test]
    fn each_stage_has_its_variables_after_the_boot_ones() {
        let variables = Variables::from(vec![
            ("ZBM_BUILDSTYLE".to_owned(), "dracut".to_owned()),
            ("BOOTFS".to_owned(), "tank/ROOT/void".to_owned()),
        ]);
        let kernel = Kernel::new(
            "tank/ROOT/void",
            "/boot/vmlinuz-6.12",
            "/boot/initramfs-6.12.img",
        );
        let env = |stage| HookEnv {
            variables: &variables,
            stage,
        };
        let boot = [("ZBM_BUILDSTYLE", "dracut"), ("BOOTFS", "tank/ROOT/void")];
        let selected = [
            ("ZBM_SELECTED_BE", "tank/ROOT/void"),
            ("ZBM_SELECTED_KERNEL", "/boot/vmlinuz-6.12"),
            ("ZBM_SELECTED_INITRAMFS", "/boot/initramfs-6.12.img"),
        ];
        let with = |extra: &[(&'static str, &'static str)]| {
            boot.iter().chain(extra).copied().collect::<Vec<_>>()
        };
        assert_eq!(
            env(StageEnv::EarlySetup).vars(),
            with(&[("ZFSBOOTMENU_INITIALIZATION", "yes")])
        );
        assert_eq!(env(StageEnv::Setup).vars(), with(&[]));
        assert_eq!(
            env(StageEnv::LoadKey {
                fs: "tank/ROOT/void",
                root: "tank"
            })
            .vars(),
            with(&[
                ("ZBM_LOCKED_FS", "tank/ROOT/void"),
                ("ZBM_ENCRYPTION_ROOT", "tank")
            ])
        );
        let mut boot_sel = selected.to_vec();
        boot_sel.push(("ZBM_SELECTED_MOUNTPOINT", "/mnt"));
        assert_eq!(
            env(StageEnv::BootSel {
                selected: &kernel,
                mountpoint: "/mnt"
            })
            .vars(),
            with(&boot_sel)
        );
        assert_eq!(
            env(StageEnv::Teardown { selected: &kernel }).vars(),
            with(&selected)
        );
        for (stage, expected) in [
            (StageEnv::EarlySetup, HookStage::EarlySetup),
            (StageEnv::Setup, HookStage::Setup),
            (
                StageEnv::Teardown { selected: &kernel },
                HookStage::Teardown,
            ),
        ] {
            assert_eq!(stage.stage(), expected);
        }
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
                &fake.context(),
                Path::new(HOOKS),
                &bare(StageEnv::EarlySetup),
                true,
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

    /// A hook is told by its first bytes, read through the files port: a text
    /// without `#!` runs on the shell, here Bash, as no shell was chosen, and
    /// an ELF hook as it is (fixes 21 and 22).
    #[test]
    fn a_hook_runs_as_its_first_bytes_say() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.files
            .file_with_mode("/libexec/hooks/setup.d/10-plain", "echo hi\n", 0o755)
            .file_with_mode("/libexec/hooks/setup.d/20-elf", "\x7fELF\x02", 0o755);
        assert!(
            run_stage(
                &fake.context(),
                Path::new(HOOKS),
                &bare(StageEnv::Setup),
                false
            ) > 0
        );
        assert_eq!(
            fake.runner.lines(),
            [
                "/bin/bash /libexec/hooks/setup.d/10-plain",
                "/libexec/hooks/setup.d/20-elf"
            ]
        );
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
            #[allow(
                clippy::disallowed_macros,
                reason = "the test harness shows a skipped test's standard error"
            )]
            {
                eprintln!("skipped: the legacy code runs as root");
            }
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
