//! Loading the key of a locked encryption root, in the order the legacy
//! runtime tries its hooks, prompts and key files.

use std::path::{Path, PathBuf};

use zbm_platform::System;
use zbm_platform::console::Screen;
use zbm_state::State;
use zbm_types::{DatasetName, HookStage};
use zbm_zfs::ZfsText;

use super::{BootPath, cache, keysource, stripped};
use crate::boot::{locked_root, zfs_value};
use crate::error::BootError;
use crate::hooks;

/// Whether the screen is cleared before a passphrase is asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Clear {
    /// Clear it, as the boot of a chosen kernel does.
    Yes,
    /// Leave it, as the countdown's boot does after clearing it itself.
    No,
}

/// How a key is loaded: where the hooks are, how the boot was chosen, and
/// whether the screen is cleared.
#[derive(Clone, Copy, Debug)]
pub struct KeyLoad<'p> {
    /// Where the hook directories are.
    pub hooks: &'p Path,
    /// How the boot was chosen: only a boot chosen in the menu tries key
    /// sources (`NO_CACHE=1` on the countdown).
    pub path: BootPath,
    /// Whether the screen is cleared before a passphrase prompt.
    pub clear: Clear,
}

/// Where the key file of a locked root is, if anywhere.
enum KeyPath {
    /// Copying from the key source unlocked the root already.
    Unlocked,
    /// The key file.
    File(PathBuf),
    /// No key file.
    Missing,
}

fn not_loaded(root: &DatasetName) -> BootError {
    BootError::KeyNotLoaded {
        root: root.to_string(),
    }
}

/// Asks for the passphrase of `root` on the console, after clearing the
/// screen when `clear` says so.
fn prompt(system: &System<'_>, root: &DatasetName, clear: Clear) -> Result<(), BootError> {
    if clear == Clear::Yes {
        system.console.screen(Screen::Clear);
        system.console.screen(Screen::MoveTo { row: 0, column: 0 });
    }
    zbm_trace::debug!("prompting for passphrase for {root}"; %root);
    let loaded = ZfsText::new(system.runner).load_key(root, "prompt");
    if loaded.unwrap_or(false) {
        Ok(())
    } else {
        Err(not_loaded(root))
    }
}

/// Runs the `load-key.d` hooks for `fs`, whose encryption root `root` is
/// locked; true when one ran and the root is unlocked now.
fn is_unlocked_by_hooks(
    system: &System<'_>,
    hooks_root: &Path,
    fs: &DatasetName,
    root: &DatasetName,
) -> bool {
    let env = [
        ("ZBM_LOCKED_FS", fs.as_str()),
        ("ZBM_ENCRYPTION_ROOT", root.as_str()),
    ];
    if hooks::run_stage(system, hooks_root, HookStage::LoadKey, false, &env) > 0
        && locked_root(system.runner, fs).is_none()
    {
        zbm_trace::debug!("fs {fs} unlocked by user hooks"; %fs);
        return true;
    }
    false
}

/// The key file for `fs` at `keylocation`: in the image or, on a boot
/// chosen in the menu, copied from the key source of `fs`.
fn key_path(
    system: &System<'_>,
    state: &State<'_>,
    how: &KeyLoad<'_>,
    fs: &DatasetName,
    root: &DatasetName,
    keylocation: &str,
) -> KeyPath {
    let key = stripped(keylocation);
    let in_image = PathBuf::from(format!("/{key}"));
    if system.files.exists(&in_image) {
        return KeyPath::File(in_image);
    }
    let source = keysource(system.runner, fs).filter(|_| how.path == BootPath::Menu);
    let Some(source) = source else {
        return KeyPath::Missing;
    };
    // It may be cached already, so whether this copies it does not matter.
    cache::cache_key(system, state, how, fs, keylocation);
    if locked_root(system.runner, fs).is_none() {
        zbm_trace::debug!("cache attempt has unlocked {root}"; %root);
        return KeyPath::Unlocked;
    }
    let cached = PathBuf::from(format!(
        "{}/{key}",
        state.key_cache(source.as_str()).display()
    ));
    if !system.files.exists(&cached) {
        return KeyPath::Missing;
    }
    zbm_trace::debug!("cached key path for {fs} is {cached}"; %fs, cached = %cached.display());
    KeyPath::File(cached)
}

/// Loads the key of `root`, the encryption root of `fs`, from `keypath`
/// if it is still locked; true when that unlocked it.
fn is_loaded_from(
    system: &System<'_>,
    fs: &DatasetName,
    root: &DatasetName,
    keypath: &Path,
) -> bool {
    let location = format!("file://{}", keypath.display());
    let loaded = locked_root(system.runner, fs).is_some()
        && ZfsText::new(system.runner)
            .load_key(root, &location)
            .unwrap_or(false);
    if loaded {
        zbm_trace::debug!(
            "unlocked {root} from key at {keypath}";
            %root, keypath = %keypath.display()
        );
    }
    loaded
}

/// Loads the key of the encryption root of `fs` if it is locked: first the
/// `load-key.d` hooks, which may unlock it themselves; then the prompt of a
/// `prompt` location; then a key file in the image or, on a boot chosen in
/// the menu, one copied from the key source; and last the passphrase prompt
/// of a `passphrase` key.
///
/// # Errors
///
/// [`BootError::KeyNotLoaded`] when the root is still locked.
pub fn load_key(
    system: &System<'_>,
    state: &State<'_>,
    how: &KeyLoad<'_>,
    fs: &DatasetName,
) -> Result<(), BootError> {
    zbm_trace::debug!("fs set to {fs}"; %fs);
    let Some(root) = locked_root(system.runner, fs) else {
        return Ok(());
    };
    if is_unlocked_by_hooks(system, how.hooks, fs, &root) {
        return Ok(());
    }
    let keylocation = zfs_value(system.runner, "keylocation", &root).unwrap_or_else(|| {
        zbm_trace::debug!("failed to read keylocation on {root}"; %root);
        "prompt".to_owned()
    });
    if keylocation == "prompt" {
        return prompt(system, &root, how.clear);
    }
    match key_path(system, state, how, fs, &root, &keylocation) {
        KeyPath::Unlocked => return Ok(()),
        KeyPath::File(keypath) if is_loaded_from(system, fs, &root, &keypath) => return Ok(()),
        KeyPath::File(_) | KeyPath::Missing => {}
    }
    let keyformat = zfs_value(system.runner, "keyformat", &root).unwrap_or_default();
    if keyformat != "passphrase" {
        zbm_trace::debug!(
            "unable to load key with format {keyformat} for {root}";
            %keyformat, %root
        );
        return Err(not_loaded(&root));
    }
    prompt(system, &root, how.clear)
}

#[cfg(test)]
mod tests;
