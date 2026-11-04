//! The start-up stage `init.d/40-early-hooks`: hooks imported from
//! `zbm.hookroot`, hooks skipped with `zbm.skip_hooks`, and the
//! `early-setup.d` hooks, which run once.
//!
//! The hooks imported are measured into PCR 9 before any hook runs
//! (ADR 0009): a TPM2 token bound to PCR 9 then unseals only with the
//! hooks it was enrolled with, so a hook root someone else put there does
//! not get the keys of a machine that unlocks itself.

use std::path::Path;

use zbm_app::{auth, steps};
use zbm_boot::measured::{self, Event, Extended};
use zbm_bootenv::hooks::{HookEnv, StageEnv, Variables, run_stage};
use zbm_platform::console::Screen;
use zbm_platform::files::Files;
use zbm_platform::{Context, System};
use zbm_state::State;
use zbm_tools::mount::Mount;
use zbm_tools::programs;

use crate::error::StartupError;

/// `zbm.hookroot`: copies the hooks of each stage directory of
/// `<filesystem>//<path>` into `root`. The file
/// system is mounted read-only on `.external_hooks` while they are copied.
/// Failures to copy single hooks are logged, and the others still copied.
/// The hooks copied, as `<stage>/<name>` and their copies.
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
) -> Result<Vec<(String, std::path::PathBuf)>, StartupError> {
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
    let copied = copy_stages(system, &mounted.path().join(path), root);
    drop(mounted);
    measure_hooks(system, state, &copied);
    Ok(copied)
}

/// Measures the hooks `copied` from the hook root, as `<stage>/<name>` and
/// the copy that runs, into PCR 9, when the machine has a TPM; a copy that
/// cannot be read is disabled, so that no hook runs unmeasured.
fn measure_hooks(system: &System<'_>, state: &State<'_>, copied: &[(String, std::path::PathBuf)]) {
    if copied.is_empty() {
        return;
    }
    if !measured::has_tpm(&Context::new(*system)) {
        zbm_trace::debug!("no TPM: the hook root is not measured");
        return;
    }
    let mut events = Vec::new();
    for (name, path) in copied {
        match system.files.read(path) {
            Ok(contents) => events.push(Event::hook(name, &contents)),
            Err(error) => {
                zbm_trace::warn!(
                    "unable to read the hook {name} to measure it ({error}): it is disabled";
                    %name, %error
                );
                let _ = system.files.set_mode(path, 0);
            }
        }
    }
    if measured::extend_events(&Context::new(*system), state, &events) == Extended::Extended {
        let count = events.len();
        zbm_trace::info!("measured {count} hooks of the hook root into PCR 9"; %count);
    }
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
/// copied. The hooks copied, as `<stage>/<name>` and their copies, in
/// order.
fn copy_stages(
    system: &System<'_>,
    source: &Path,
    root: &Path,
) -> Vec<(String, std::path::PathBuf)> {
    let mut copied = Vec::new();
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
            let file = source_dir.join(&name);
            if !system.files.is_file(&file) {
                continue;
            }
            // `cp` itself, so that the copy gets the mode `cp` gives it.
            if programs::copy_file(system.runner, &file, &target)
                .is_ok_and(|output| output.status.is_success())
            {
                let named = format!("{}/{}", stage.to_string_lossy(), name.to_string_lossy());
                copied.push((named, target.join(&name)));
            } else {
                zbm_trace::warn!("failed to copy user hook {hook}"; hook = %file.display());
            }
        }
    }
    copied
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
    let locked = auth::Lock::of(system.files).is_on();
    // Lockdown is known from the start, as is Secure Boot (ADR 0017).
    zbm_boot::mount_securityfs(&Context::new(*system));
    let checked = zbm_tools::kexec::checked_boot(system.files).is_some();
    if locked && !checked {
        zbm_trace::warn!(
            "the menu is locked, but neither lockdown nor Secure Boot is on: \
             a changed kernel command line gets past the lock"
        );
    }
    let imported = if hook_root.is_empty() {
        Ok(())
    } else if locked {
        // Whoever names a hook root runs code before the menu: a locked
        // image takes none (ROADMAP.md SE9).
        zbm_trace::warn!("the menu is locked: zbm.hookroot={hook_root} is ignored"; %hook_root);
        Ok(())
    } else {
        import_hooks(system, state, root, hook_root).map(|copied| {
            if checked {
                keep_steps(system.files, &copied);
            }
        })
    };
    if let Some(skip) = skip {
        skip_hooks(system.files, root, skip);
        for name in steps::skip(state, skip, locked || checked) {
            zbm_trace::warn!(
                "zbm.skip_hooks names {name}, which this locked or measured machine runs anyway";
                name
            );
        }
    }
    imported
}

/// Under lockdown or Secure Boot, disables each hook of the hook root,
/// `copied`, that would replace a step this machine keeps, the LUKS close
/// (ADR 0017, point 4): the step then runs.
fn keep_steps(files: &dyn Files, copied: &[(String, std::path::PathBuf)]) {
    let kept = steps::kept();
    for (name, path) in copied {
        if kept.contains(name) {
            zbm_trace::warn!(
                "the hook root's {name} would replace a step this machine keeps: it is disabled";
                %name
            );
            let _ = files.set_mode(path, 0);
        }
    }
}

/// Runs the `early-setup.d` hooks below `root` once, on a cleared screen,
/// with `variables`, and the built-in steps of the stage that did not run
/// yet, which `state` then records.
pub fn run_early_stage(system: &System<'_>, state: &State<'_>, root: &Path, variables: &Variables) {
    system.console.screen(Screen::Clear);
    let env = HookEnv {
        variables,
        stage: StageEnv::EarlySetup,
    };
    let steps = steps::built_in(state);
    run_stage(&Context::new(*system), root, &env, &steps, true);
    steps::record_ran(state, &steps);
}

#[cfg(test)]
mod tests;
