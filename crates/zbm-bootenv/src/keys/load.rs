//! Loading the key of a locked encryption root, in the order the legacy
//! runtime tries its hooks, prompts and key files.

use std::path::{Path, PathBuf};

use zbm_platform::Context;
use zbm_platform::interaction::{Interaction, SecretKind, SecretRequest, SecretSubject};
use zbm_state::State;
use zbm_types::{DatasetName, HookStage};
use zbm_zfs::ZfsText;

use super::{BootPath, cache, keysource, stripped};
use crate::error::BootEnvError;
use crate::hooks;
use crate::locked_root;
use crate::zfs::zfs_value;

/// How many times a key typed for an encryption root is tried, as `zfs
/// load-key` tries one typed on its terminal.
pub(crate) const KEY_TRIES: u32 = 3;

/// How a key is loaded: where the hooks are, and how the boot was chosen.
#[non_exhaustive]
#[derive(Clone, Copy, Debug)]
pub struct KeyLoad<'p> {
    /// Where the hook directories are.
    pub hooks: &'p Path,
    /// How the boot was chosen: only a boot chosen in the menu tries key
    /// sources (`NO_CACHE=1` on the countdown).
    pub path: BootPath,
}

impl<'p> KeyLoad<'p> {
    /// A key loaded with the hooks below `hooks`, for a boot chosen by `path`.
    pub fn new(hooks: &'p Path, path: BootPath) -> KeyLoad<'p> {
        KeyLoad { hooks, path }
    }
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

fn not_loaded(root: &DatasetName) -> BootEnvError {
    BootEnvError::KeyNotLoaded {
        root: root.to_string(),
    }
}

/// Asks `interaction` for the key of `root`, whose `keyformat` is
/// `keyformat`, and loads what was typed, [`KEY_TRIES`] times at most. A
/// raw key is never typed: `zfs load-key` reads one from a file only.
fn prompt(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    root: &DatasetName,
    keyformat: &str,
) -> Result<(), BootEnvError> {
    let kind = match keyformat {
        "hex" => SecretKind::HexKey,
        "raw" => {
            zbm_trace::error!("a raw key for {root} cannot be typed"; %root);
            return Err(not_loaded(root));
        }
        _ => SecretKind::Passphrase,
    };
    zbm_trace::debug!("prompting for passphrase for {root}"; %root);
    let zfs = ZfsText::new(context.runner());
    for attempt in 1..=KEY_TRIES {
        let request = SecretRequest {
            subject: SecretSubject::EncryptionRoot(root.as_str()),
            kind: kind.clone(),
            attempt,
            tries: Some(KEY_TRIES),
            opened_elsewhere: None,
            description: None,
        };
        // Nothing more can be typed.
        let Some(key) = interaction.secret(&request).secret() else {
            break;
        };
        if zfs.load_typed_key(root, key).unwrap_or(false) {
            return Ok(());
        }
    }
    Err(not_loaded(root))
}

/// Runs the `load-key.d` hooks for `fs`, whose encryption root `root` is
/// locked; true when one ran and the root is unlocked now.
fn is_unlocked_by_hooks(
    context: &Context<'_>,
    hooks_root: &Path,
    fs: &DatasetName,
    root: &DatasetName,
) -> bool {
    let env = [
        ("ZBM_LOCKED_FS", fs.as_str()),
        ("ZBM_ENCRYPTION_ROOT", root.as_str()),
    ];
    if hooks::run_stage(context, hooks_root, HookStage::LoadKey, false, &env) > 0
        && locked_root(context.runner(), fs).is_none()
    {
        zbm_trace::debug!("fs {fs} unlocked by user hooks"; %fs);
        return true;
    }
    false
}

/// The key file for `fs` at `keylocation`: in the image or, on a boot
/// chosen in the menu, copied from the key source of `fs`.
fn key_path(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    state: &State<'_>,
    how: &KeyLoad<'_>,
    fs: &DatasetName,
    root: &DatasetName,
    keylocation: &str,
) -> KeyPath {
    let key = stripped(keylocation);
    let in_image = PathBuf::from(format!("/{key}"));
    if context.files().exists(&in_image) {
        return KeyPath::File(in_image);
    }
    let source = keysource(context.runner(), fs).filter(|_| how.path == BootPath::Menu);
    let Some(source) = source else {
        return KeyPath::Missing;
    };
    // It may be cached already, so whether this copies it does not matter.
    cache::cache_key(context, interaction, state, how, fs, keylocation);
    if locked_root(context.runner(), fs).is_none() {
        zbm_trace::debug!("cache attempt has unlocked {root}"; %root);
        return KeyPath::Unlocked;
    }
    let cached = PathBuf::from(format!(
        "{}/{key}",
        state.key_cache(source.as_str()).display()
    ));
    if !context.files().exists(&cached) {
        return KeyPath::Missing;
    }
    zbm_trace::debug!("cached key path for {fs} is {cached}"; %fs, cached = %cached.display());
    KeyPath::File(cached)
}

/// Loads the key of `root`, the encryption root of `fs`, from `keypath`
/// if it is still locked; true when that unlocked it.
fn is_loaded_from(
    context: &Context<'_>,
    fs: &DatasetName,
    root: &DatasetName,
    keypath: &Path,
) -> bool {
    let location = format!("file://{}", keypath.display());
    let loaded = locked_root(context.runner(), fs).is_some()
        && ZfsText::new(context.runner())
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
/// [`BootEnvError::KeyNotLoaded`] when the root is still locked.
pub fn load_key(
    context: &Context<'_>,
    interaction: &dyn Interaction,
    state: &State<'_>,
    how: &KeyLoad<'_>,
    fs: &DatasetName,
) -> Result<(), BootEnvError> {
    zbm_trace::debug!("fs set to {fs}"; %fs);
    let Some(root) = locked_root(context.runner(), fs) else {
        return Ok(());
    };
    if is_unlocked_by_hooks(context, how.hooks, fs, &root) {
        return Ok(());
    }
    let keylocation = zfs_value(context.runner(), "keylocation", &root).unwrap_or_else(|| {
        zbm_trace::debug!("failed to read keylocation on {root}"; %root);
        "prompt".to_owned()
    });
    let keyformat = || zfs_value(context.runner(), "keyformat", &root).unwrap_or_default();
    if keylocation == "prompt" {
        return prompt(context, interaction, &root, &keyformat());
    }
    match key_path(context, interaction, state, how, fs, &root, &keylocation) {
        KeyPath::Unlocked => return Ok(()),
        KeyPath::File(keypath) if is_loaded_from(context, fs, &root, &keypath) => return Ok(()),
        KeyPath::File(_) | KeyPath::Missing => {}
    }
    let keyformat = keyformat();
    if keyformat != "passphrase" {
        zbm_trace::debug!(
            "unable to load key with format {keyformat} for {root}";
            %keyformat, %root
        );
        return Err(not_loaded(&root));
    }
    prompt(context, interaction, &root, &keyformat)
}

#[cfg(test)]
mod tests;
