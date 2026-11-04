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
//! `BOOTFS` once it is known), then the stage's own, and `ZBM_HOOK_API=2`
//! with `ZBM_HOOK_CONTEXT`, the file that says what the stage knows
//! (ADR 0017, point 8).

use std::path::{Path, PathBuf};

use zbm_platform::Context;
use zbm_platform::files::{FileKind, Files};
use zbm_tools::programs;
use zbm_types::{DatasetName, HookStage};

use crate::error::BootEnvError;
use crate::kernels::Kernel;
use zbm_types::code::Coded as _;

mod context;

use context::HOOK_API;
pub use context::Loaded;

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
        /// What `kexec` loaded, when the process that runs the stage
        /// loaded it.
        loaded: Option<Loaded<'a>>,
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
    /// with `ZBM_SELECTED_MOUNTPOINT`, and in `teardown.d`; and in every
    /// stage `ZBM_HOOK_API`, the version of the hook API.
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
            StageEnv::Teardown {
                selected: kernel, ..
            } => vars.extend(selected(kernel)),
        }
        vars.push(("ZBM_HOOK_API", HOOK_API));
        vars
    }
}

/// A built-in step of a hook stage, which runs where the hook of its name
/// ran (ADR 0017): its name is that hook's file name, `80-luks-close.sh`, so
/// it runs in the order of the names, among the stage's hooks, and a hook of
/// its name replaces it.
pub trait BootStep: Sync + std::fmt::Debug {
    /// The step's name, its hook's file name.
    fn name(&self) -> &'static str;

    /// The stage it runs in.
    fn stage(&self) -> HookStage;

    /// Whether `zbm.skip_hooks` may not skip it, nor a hook root replace
    /// it, on a locked or measured machine: the LUKS close.
    fn is_kept_when_guarded(&self) -> bool {
        false
    }

    /// Runs the step with the stage's environment `env`.
    ///
    /// # Errors
    ///
    /// [`BootEnvError::Step`] with what went wrong; the stage goes on.
    fn run(&self, context: &Context<'_>, env: &HookEnv<'_>) -> Result<(), BootEnvError>;
}

/// The built-in steps a boot runs, which compare and print by their names.
#[derive(Clone, Default)]
pub struct BootSteps(Vec<&'static dyn BootStep>);

impl From<Vec<&'static dyn BootStep>> for BootSteps {
    fn from(steps: Vec<&'static dyn BootStep>) -> BootSteps {
        BootSteps(steps)
    }
}

impl std::fmt::Debug for BootSteps {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_list()
            .entries(self.0.iter().map(|step| step.name()))
            .finish()
    }
}

impl PartialEq for BootSteps {
    fn eq(&self, other: &BootSteps) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(&other.0)
                .all(|(one, two)| one.name() == two.name())
    }
}

impl Eq for BootSteps {}

impl BootSteps {
    /// The steps that `keep` keeps.
    #[must_use]
    pub fn retain(mut self, keep: impl Fn(&dyn BootStep) -> bool) -> BootSteps {
        self.0.retain(|step| keep(*step));
        self
    }

    /// Each step, in the table's order.
    pub fn iter(&self) -> impl Iterator<Item = &'static dyn BootStep> + '_ {
        self.0.iter().copied()
    }
}

/// A hook of a stage, or a built-in step.
enum Runs {
    /// A hook file.
    Hook(PathBuf),
    /// A built-in step.
    Step(&'static dyn BootStep),
}

impl Runs {
    /// The name the stage's order goes by.
    fn name(&self) -> Vec<u8> {
        match self {
            Runs::Hook(path) => path
                .file_name()
                .map(|name| name.as_encoded_bytes().to_vec())
                .unwrap_or_default(),
            Runs::Step(step) => step.name().as_bytes().to_vec(),
        }
    }
}

/// How many bytes of a hook tell how it runs.
const START: usize = 256;

/// Runs the hooks of `env`'s stage below `root`, as `zfsbootmenu-run-hooks`
/// does, with the standard streams of this process and its environment plus
/// `env`'s variables, and `steps` of that stage among them, in the order of
/// their names; a hook of a step's name runs in its place. With `once`, each
/// hook then loses its permissions and gets the suffix `.completed`. When a
/// hook runs, the stage's context file is written first, named by
/// `ZBM_HOOK_CONTEXT`, and removed after the stage. Returns how many hooks
/// and steps ran; a missing directory runs no hook.
pub fn run_stage(
    context: &Context<'_>,
    root: &Path,
    env: &HookEnv<'_>,
    steps: &BootSteps,
    once: bool,
) -> usize {
    let stage = env.stage.stage();
    let hooks = if context.files().is_dir(&root.join(stage.as_str())) {
        executable_hooks(context.files(), root, stage)
    } else {
        zbm_trace::debug!("no hook directory for {stage}"; %stage);
        Vec::new()
    };
    let mut runs: Vec<Runs> = hooks.into_iter().map(Runs::Hook).collect();
    for step in steps.iter().filter(|step| step.stage() == stage) {
        if runs.iter().any(|run| run.name() == step.name().as_bytes()) {
            let name = step.name();
            zbm_trace::info!("the hook {name} replaces its built-in step"; name);
            continue;
        }
        runs.push(Runs::Step(step));
    }
    runs.sort_by_key(Runs::name);
    let mut vars = env.vars();
    let has_hooks = runs.iter().any(|run| matches!(run, Runs::Hook(_)));
    let written = if has_hooks {
        context::write(context, env)
    } else {
        None
    };
    let written_text = written.as_ref().map(|path| path.to_string_lossy());
    if let Some(path) = &written_text {
        vars.push(("ZBM_HOOK_CONTEXT", path));
    }
    for run in &runs {
        match run {
            Runs::Hook(hook) => run_hook(context, hook, &vars, once),
            Runs::Step(step) => {
                let name = step.name();
                zbm_trace::info!("running the built-in step {name}"; name);
                if let Err(error) = step.run(context, env) {
                    zbm_trace::error!("{error}"; %error, code = %error.code());
                }
            }
        }
    }
    if let Some(path) = &written {
        context::remove(context, path);
    }
    runs.len()
}

/// Runs the hook `hook` with `vars`; with `once`, it then loses its
/// permissions and gets the suffix `.completed`.
fn run_hook(context: &Context<'_>, hook: &Path, vars: &[(&str, &str)], once: bool) {
    zbm_trace::info!("processing {hook}"; hook = %hook.display());
    let start = context.files().read_prefix(hook, START).ok();
    programs::run_hook(context.runner(), hook, start.as_deref(), vars);
    if once {
        zbm_trace::info!("Disabling hook after execution: {hook}"; hook = %hook.display());
        let _ = context.files().set_mode(hook, 0);
        let mut completed = hook.to_path_buf().into_os_string();
        completed.push(".completed");
        let _ = context.files().rename(hook, Path::new(&completed));
    }
}

#[cfg(test)]
mod tests;
