//! The hook API version 2 (ADR 0017, point 8): `ZBM_HOOK_API=2` in every
//! stage, and `ZBM_HOOK_CONTEXT`, the path of a JSON file the runtime
//! writes before the stage's hooks run and removes after them, which says
//! what the stage knows. It holds no secret and no key location's
//! contents; fields may be added, never changed.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zbm_platform::Context;
use zbm_platform::files::{FileKind, Files};

use super::{HookEnv, StageEnv};
use crate::kernels::Kernel;

/// The version of the hook API, `ZBM_HOOK_API`.
pub(super) const HOOK_API: &str = "2";

/// The directory of the context files, one per stage:
/// `/run/zbm/hooks/<stage>.json`.
pub(super) const CONTEXTS: &str = "/run/zbm/hooks";

/// Where the kernel lists a statistics directory per imported pool.
const KSTAT: &str = "/proc/spl/kstat/zfs";

/// What `kexec` loaded, which the context of `teardown.d` says.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Loaded<'a> {
    /// The kernel's command line.
    pub command_line: &'a str,
    /// The device tree, if one was loaded.
    pub devicetree: Option<&'a Path>,
    /// What loaded it: `kexec_file_load`, the kernel's call, or `kexec`,
    /// the program.
    pub by: &'a str,
}

/// The value of the variable `name` of `env`, if it is set and not empty.
fn var<'e>(env: &'e HookEnv<'_>, name: &str) -> Option<&'e str> {
    env.variables
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
}

/// The pools the kernel has imported, by name, in order.
fn imported_pools(files: &dyn Files) -> Vec<String> {
    let kstat = Path::new(KSTAT);
    let mut pools: Vec<String> = files
        .read_dir(kstat)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| {
            files
                .metadata(&kstat.join(name))
                .is_ok_and(|metadata| metadata.kind == FileKind::Directory)
        })
        .filter_map(|name| name.into_string().ok())
        .collect();
    pools.sort();
    pools
}

/// The chosen boot environment and kernel, as the context says them.
fn selected(kernel: &Kernel) -> Value {
    json!({
        "boot_environment": kernel.dataset,
        "kernel": kernel.kernel,
        "initramfs": kernel.initramfs,
    })
}

/// The context of `env`'s stage on the machine of `context`.
pub(super) fn document(context: &Context<'_>, env: &HookEnv<'_>) -> Value {
    let files = context.files();
    let stage = env.stage.stage();
    let mut document = json!({
        "api": 2,
        "stage": stage.as_str(),
        "runtime": var(env, "ZBM_RUNTIME"),
        "build_style": var(env, "ZBM_BUILDSTYLE"),
        "release": var(env, "ZBM_RELEASE_BUILD").is_some(),
        "locked": files.exists(Path::new(zbm_config::conf::LOCK)),
        "lockdown": zbm_tools::kexec::lockdown(files),
        "secure_boot": zbm_tools::kexec::is_secure_boot(files),
    });
    let mut known = |name: &str, value: Value| {
        if let Some(object) = document.as_object_mut() {
            object.insert(name.to_owned(), value);
        }
    };
    if !matches!(env.stage, StageEnv::EarlySetup) {
        known("pools", json!(imported_pools(files)));
        known("boot_environment", json!(var(env, "BOOTFS")));
    }
    match env.stage {
        StageEnv::EarlySetup | StageEnv::Setup => {}
        StageEnv::LoadKey { fs, root } => {
            known("locked_fs", json!(fs));
            known("encryption_root", json!(root));
        }
        StageEnv::BootSel {
            selected: kernel,
            mountpoint,
        } => {
            known("selected", selected(kernel));
            known("mountpoint", json!(mountpoint));
        }
        StageEnv::Teardown {
            selected: kernel,
            loaded,
        } => {
            known("selected", selected(kernel));
            known(
                "command_line",
                json!(loaded.map(|loaded| loaded.command_line)),
            );
            let tree = loaded.and_then(|loaded| loaded.devicetree);
            known("devicetree", json!(tree.map(|tree| tree.to_string_lossy())));
            known("loaded_by", json!(loaded.map(|loaded| loaded.by)));
        }
    }
    document
}

/// The context file of `env`'s stage.
pub(super) fn path_of(env: &HookEnv<'_>) -> PathBuf {
    Path::new(CONTEXTS).join(format!("{}.json", env.stage.stage()))
}

/// Writes the context of `env`'s stage to its file, readable by everyone
/// and writable by no one: its path, or `None` when it could not be
/// written, which is logged, and what was written removed.
pub(super) fn write(context: &Context<'_>, env: &HookEnv<'_>) -> Option<PathBuf> {
    let files = context.files();
    let path = path_of(env);
    let mut text = document(context, env).to_string();
    text.push('\n');
    let written = files
        .create_dir_all(Path::new(CONTEXTS))
        .and_then(|()| {
            // A file left by a stage that did not finish goes first.
            let _ = files.remove_file(&path);
            files.write(&path, text.as_bytes())
        })
        .and_then(|()| files.set_mode(&path, 0o444));
    match written {
        Ok(()) => Some(path),
        Err(error) => {
            // What a write that failed part way left is not the context.
            let _ = files.remove_file(&path);
            let path = path.display();
            zbm_trace::warn!("unable to write the hook context {path}: {error}"; %path, %error);
            None
        }
    }
}

/// Removes the context file `path` after its stage.
pub(super) fn remove(context: &Context<'_>, path: &Path) {
    if let Err(error) = context.files().remove_file(path) {
        let path = path.display();
        zbm_trace::warn!("unable to remove the hook context {path}: {error}"; %path, %error);
    }
}

#[cfg(test)]
mod tests;
