//! The registry of key methods (ROADMAP.md DM6).
//!
//! Each way of getting a volume's key is a [`KeyMethod`]; a [`Registry`]
//! tries its methods in order until one opens the volume. The standard
//! one ([`Registry::standard`]) is the legacy order: a key script, a key
//! file (decrypting a GnuPG key file), the LUKS2 tokens, with the handlers
//! of their types in it ([`crate::keys::handlers`]), passphrases that
//! opened other volumes, YubiKey challenge-response, and the passphrase
//! prompt last; another lists fewer, such as the key file alone of `zluks
//! open --key-file`.
//!
//! A method gets the volume closed and gives back the mapping it opened,
//! which only the LUKS port makes, or why it did not ([`Refusal`]). Every
//! attempt is kept ([`Attempt`]): with the volume when none opened it
//! ([`NotUnlocked`]), and in the run ([`Unlocker::attempts`]).

use std::path::Path;
use std::rc::Rc;

use tracing::{debug, info, info_span};
use zbm_crypt::Closed;
use zbm_tools::programs;

use crate::error::LuksError;
use crate::keys::OpenMapping;
use crate::keys::gpg::is_gpg_keyfile;
use crate::unlock::Method;
use crate::unlocker::{Key, Unlocker};
use crate::volume::Volume;
use zbm_crypt::Open;

/// Why a key method did not open a volume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// It does not apply to the volume: no key script, no key file, tokens
    /// turned off.
    NotApplicable,
    /// Another program opened the volume while it tried, such as `zluks`
    /// over SSH: nothing more is tried.
    Elsewhere,
    /// It tried, and failed for this reason.
    Failed(LuksError),
}

/// One key method tried on a volume, and what came of it: `None` when it
/// opened the volume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attempt {
    /// The method.
    pub method: Method,
    /// Why it did not open the volume, if it did not.
    pub refusal: Option<Refusal>,
}

/// One way of getting the key of a volume and opening it.
pub trait KeyMethod {
    /// The method, as attempts and [`crate::unlock::Opened`] report it.
    fn method(&self) -> Method;

    /// Opens `volume`, with the secrets that earlier volumes of the run
    /// left in `unlocker`.
    ///
    /// # Errors
    ///
    /// Why it did not.
    fn try_open(
        &self,
        unlocker: &mut Unlocker<'_>,
        volume: &Volume<Closed>,
    ) -> Result<OpenMapping, Refusal>;
}

/// What the methods of a [`Registry`] came to, when the volume is open.
#[derive(Debug)]
pub enum Unlocked {
    /// A method opened it.
    Opened(Volume<Open>, Method),
    /// Another program opened it meanwhile.
    Elsewhere,
}

/// No method opened the volume: it, still closed, and every attempt in
/// order.
#[derive(Debug)]
pub struct NotUnlocked {
    /// The volume.
    pub volume: Volume<Closed>,
    /// Each method tried, with why it did not open the volume.
    pub attempts: Vec<Attempt>,
}

impl NotUnlocked {
    /// The error it comes to: why the last method that could have opened
    /// the volume gave up when that ends the attempts, as the passphrase
    /// prompt does when the volume is skipped or there is no console;
    /// otherwise that no method opened it, with every attempt.
    pub fn into_error(self) -> LuksError {
        let ending = self
            .attempts
            .last()
            .and_then(|attempt| match &attempt.refusal {
                Some(Refusal::Failed(
                    error @ (LuksError::Skipped { .. } | LuksError::NoConsole { .. }),
                )) => Some(error.clone()),
                _ => None,
            });
        ending.unwrap_or_else(|| LuksError::NotUnlocked {
            device: self.volume.device().to_string(),
            attempts: self.attempts,
        })
    }
}

/// Key methods, tried in order until one opens a volume.
#[derive(Clone)]
pub struct Registry {
    methods: Rc<[Rc<dyn KeyMethod>]>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.methods()).finish()
    }
}

impl Registry {
    /// The methods of `methods`, in this order.
    pub fn of(methods: Vec<Rc<dyn KeyMethod>>) -> Registry {
        Registry {
            methods: methods.into(),
        }
    }

    /// The legacy order: a key script, a key file, the LUKS2 tokens,
    /// passphrases that opened other volumes, YubiKey challenge-response,
    /// and the passphrase prompt.
    pub fn standard() -> Registry {
        Registry::of(vec![
            Rc::new(KeyScript),
            Rc::new(KeyFile),
            Rc::new(Tokens),
            Rc::new(CachedPassphrases),
            Rc::new(YubiKey),
            Rc::new(Passphrase),
        ])
    }

    /// The key file alone, as `zluks open --key-file` opens a volume.
    pub fn key_file() -> Registry {
        Registry::of(vec![Rc::new(KeyFile)])
    }

    /// The methods, in order.
    pub fn methods(&self) -> impl Iterator<Item = Method> + '_ {
        self.methods.iter().map(|method| method.method())
    }

    /// Opens `volume` with the first method that can, each in a
    /// `key_method` span, and keeps every attempt in `unlocker`.
    ///
    /// # Errors
    ///
    /// [`NotUnlocked`] with the volume, still closed, and every attempt.
    pub fn unlock(
        &self,
        unlocker: &mut Unlocker<'_>,
        volume: Volume<Closed>,
    ) -> Result<Unlocked, NotUnlocked> {
        let _span =
            info_span!("unlock", device = %volume.device(), name = %volume.name()).entered();
        let mut attempts = Vec::new();
        let mut outcome = None;
        for method in self.methods.iter() {
            let id = method.method();
            let _method = info_span!("key_method", method = ?id).entered();
            match method.try_open(unlocker, &volume) {
                Ok(open) => {
                    info!("opened");
                    attempts.push(Attempt {
                        method: id,
                        refusal: None,
                    });
                    outcome = Some(Ok((open, id)));
                    break;
                }
                Err(Refusal::Elsewhere) => {
                    info!("opened elsewhere");
                    attempts.push(Attempt {
                        method: id,
                        refusal: Some(Refusal::Elsewhere),
                    });
                    outcome = Some(Err(()));
                    break;
                }
                Err(Refusal::NotApplicable) => debug!("not applicable"),
                Err(refusal @ Refusal::Failed(_)) => {
                    if let Refusal::Failed(error) = &refusal {
                        debug!(%error, "refused");
                    }
                    attempts.push(Attempt {
                        method: id,
                        refusal: Some(refusal),
                    });
                }
            }
        }
        unlocker.keep_attempts(volume.device(), &attempts);
        match outcome {
            Some(Ok((open, id))) => Ok(Unlocked::Opened(volume.opened(open), id)),
            Some(Err(())) => Ok(Unlocked::Elsewhere),
            None => Err(NotUnlocked { volume, attempts }),
        }
    }
}

/// A key script of the crypttab prints the key (Debian's crypttab
/// semantics); the key file field is its argument.
pub struct KeyScript;

impl KeyMethod for KeyScript {
    fn method(&self) -> Method {
        Method::KeyScript
    }

    fn try_open(
        &self,
        unlocker: &mut Unlocker<'_>,
        volume: &Volume<Closed>,
    ) -> Result<OpenMapping, Refusal> {
        let Some(script) = &volume.options().keyscript else {
            return Err(Refusal::NotApplicable);
        };
        open_with_keyscript(unlocker, volume, script).map_err(Refusal::Failed)
    }
}

/// Runs a key script with the crypttab variables and opens the volume with
/// what it prints.
fn open_with_keyscript(
    unlocker: &Unlocker<'_>,
    volume: &Volume<Closed>,
    script: &str,
) -> Result<OpenMapping, LuksError> {
    let device = volume.device();
    let executable = unlocker
        .context()
        .files()
        .metadata(Path::new(script))
        .is_ok_and(|metadata| metadata.is_executable());
    if !executable {
        zbm_trace::warn!("key script {script} is not executable"; %script);
        return Err(LuksError::KeyScriptNotExecutable {
            script: script.to_owned(),
        });
    }
    zbm_trace::info!("unlocking {device} with key script {script}"; %device, %script);
    let key = if volume.keyspec().is_empty() {
        "none"
    } else {
        volume.keyspec()
    };
    let mut env = unlocker.exported().to_vec();
    env.extend([
        ("CRYPTTAB_NAME", volume.name().as_str()),
        ("CRYPTTAB_SOURCE", device.as_str()),
        ("CRYPTTAB_KEY", key),
        ("CRYPTTAB_OPTIONS", volume.entry().options_text.as_str()),
        ("CRYPTTAB_TRIED", "0"),
    ]);
    // The script may ask on the console, which it gets for the time.
    let mut printed = None;
    unlocker.with_console(&mut |runner| {
        printed = programs::key_script(runner, script, key, &env)
            .ok()
            .and_then(zbm_platform::Output::into_secret);
    });
    let printed = printed.unwrap_or_default();
    let opened = unlocker.cryptsetup_open(volume.target(), Key::Stdin(&printed));
    if opened.is_ok() {
        zbm_trace::info!(
            "unlocked {device} as {name} with key script";
            %device, name = %volume.name()
        );
    } else {
        zbm_trace::warn!("key script {script} failed to unlock {device}"; %script, %device);
    }
    opened
}

/// A key file, possibly on a key device. A GnuPG-encrypted key file is
/// decrypted in memory and kept for the other volumes of the run. Not tried
/// with a key script, whose argument the key file field is.
pub struct KeyFile;

impl KeyMethod for KeyFile {
    fn method(&self) -> Method {
        Method::KeyFile
    }

    fn try_open(
        &self,
        unlocker: &mut Unlocker<'_>,
        volume: &Volume<Closed>,
    ) -> Result<OpenMapping, Refusal> {
        let keyspec = volume.keyspec();
        if keyspec.is_empty() || volume.options().keyscript.is_some() {
            return Err(Refusal::NotApplicable);
        }
        let cached = unlocker
            .gpg_plain
            .get(keyspec)
            .filter(|plain| !plain.is_empty())
            .map(zbm_platform::Secret::duplicate);
        let opened = match cached {
            Some(plain) => open_with_decrypted(unlocker, volume, &plain),
            None => open_with_keyfile(unlocker, volume),
        };
        let open = opened.map_err(|error| {
            zbm_trace::warn!(
                "key file {keyspec} failed to unlock {device}";
                %keyspec, device = %volume.device()
            );
            Refusal::Failed(error)
        })?;
        zbm_trace::info!(
            "unlocked {device} as {name} with key file";
            device = %volume.device(), name = %volume.name()
        );
        Ok(open)
    }
}

/// Opens `volume` with a GnuPG key file decrypted for an earlier volume.
fn open_with_decrypted(
    unlocker: &Unlocker<'_>,
    volume: &Volume<Closed>,
    plain: &zbm_platform::Secret,
) -> Result<OpenMapping, LuksError> {
    let (device, keyspec) = (volume.device(), volume.keyspec());
    zbm_trace::info!(
        "unlocking {device} with GnuPG key file {keyspec} (already decrypted)";
        %device, %keyspec
    );
    unlocker.cryptsetup_open(volume.target(), Key::Stdin(plain))
}

/// That the key file of `volume` could not be found, read or decrypted.
fn no_key_file(volume: &Volume<Closed>) -> LuksError {
    LuksError::KeyFile {
        keyspec: volume.keyspec().to_owned(),
        device: volume.device().to_string(),
    }
}

/// Finds the key file of `volume` (mounting its key device if it has one),
/// decrypts it if it is a GnuPG key file, and opens the volume with it.
fn open_with_keyfile(
    unlocker: &mut Unlocker<'_>,
    volume: &Volume<Closed>,
) -> Result<OpenMapping, LuksError> {
    let (device, keyspec) = (volume.device(), volume.keyspec());
    let named = volume.target().volume();
    let found = unlocker.find_keyfile(keyspec, named);
    let Some(path) = &found.path else {
        unlocker.release_keyfile(&found);
        return Err(no_key_file(volume));
    };
    let target = volume.target();
    let opened = if volume.options().gpg || is_gpg_keyfile(path, "") {
        match unlocker.gpg_decrypt(path, named) {
            Some(plain) => {
                zbm_trace::info!(
                    "unlocking {device} with GnuPG key file {keyspec}";
                    %device, %keyspec
                );
                let opened = unlocker.cryptsetup_open(target, Key::Stdin(&plain));
                // Kept for other volumes until the secrets are forgotten.
                unlocker.gpg_plain.insert(keyspec.to_owned(), plain);
                opened
            }
            None => Err(no_key_file(volume)),
        }
    } else {
        zbm_trace::info!("unlocking {device} with key file {keyspec}"; %device, %keyspec);
        unlocker.cryptsetup_open(target, Key::File(Path::new(path)))
    };
    unlocker.release_keyfile(&found);
    opened
}

/// LUKS2 tokens: TPM2, FIDO2 security keys and clevis, through the handlers
/// of their types, unless the crypttab or the configuration turns them off.
pub struct Tokens;

impl KeyMethod for Tokens {
    fn method(&self) -> Method {
        Method::Token
    }

    fn try_open(
        &self,
        unlocker: &mut Unlocker<'_>,
        volume: &Volume<Closed>,
    ) -> Result<OpenMapping, Refusal> {
        if volume.options().notoken || !unlocker.tokens_enabled() {
            return Err(Refusal::NotApplicable);
        }
        match unlocker.try_tokens(volume.target(), volume.header()) {
            Ok(open) => Ok(open),
            // Another tool may have unlocked the device meanwhile.
            Err(_) if unlocker.is_device_open(volume.device().as_str()) => Err(Refusal::Elsewhere),
            Err(error) => Err(Refusal::Failed(error)),
        }
    }
}

/// Passphrases (and YubiKey responses) that opened another volume.
pub struct CachedPassphrases;

impl KeyMethod for CachedPassphrases {
    fn method(&self) -> Method {
        Method::CachedPassphrase
    }

    fn try_open(
        &self,
        unlocker: &mut Unlocker<'_>,
        volume: &Volume<Closed>,
    ) -> Result<OpenMapping, Refusal> {
        if unlocker.passphrases.is_empty() {
            return Err(Refusal::NotApplicable);
        }
        let mut last = None;
        for passphrase in &unlocker.passphrases {
            match unlocker.cryptsetup_open(volume.target(), Key::Stdin(passphrase)) {
                Ok(open) => {
                    zbm_trace::info!(
                        "unlocked {device} as {name} with a previously entered passphrase";
                        device = %volume.device(), name = %volume.name()
                    );
                    return Ok(open);
                }
                Err(error) => last = Some(error),
            }
        }
        Err(last.map_or(Refusal::NotApplicable, Refusal::Failed))
    }
}

/// YubiKey challenge-response, when the crypttab asks for it.
pub struct YubiKey;

impl KeyMethod for YubiKey {
    fn method(&self) -> Method {
        Method::YubiKey
    }

    fn try_open(
        &self,
        unlocker: &mut Unlocker<'_>,
        volume: &Volume<Closed>,
    ) -> Result<OpenMapping, Refusal> {
        let Some(yubikey) = &volume.options().yubikey else {
            return Err(Refusal::NotApplicable);
        };
        unlocker
            .try_ykchalresp(volume.target(), yubikey, volume.options().tries)
            .map_err(Refusal::Failed)
    }
}

/// The passphrase prompt, until the person at the console types one that
/// opens the volume, skips it, or another program opens it.
pub struct Passphrase;

impl KeyMethod for Passphrase {
    fn method(&self) -> Method {
        Method::Passphrase
    }

    fn try_open(
        &self,
        unlocker: &mut Unlocker<'_>,
        volume: &Volume<Closed>,
    ) -> Result<OpenMapping, Refusal> {
        unlocker.ask_passphrase(volume)
    }
}

#[cfg(test)]
mod tests;
