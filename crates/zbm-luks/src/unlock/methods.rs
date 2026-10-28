//! The key methods tried before the passphrase prompt, as strategies.
//!
//! Each way of getting a volume's key is a [`KeyMethod`]; [`key_methods`]
//! lists them in the order they are tried, which is the legacy order: a key
//! script, a key file (decrypting a GnuPG key file), LUKS2 tokens,
//! passphrases that opened other volumes, and YubiKey challenge-response. A
//! method that does not apply to a volume, or whose key does not open it,
//! answers [`Tried::NotOpened`], and the next one is tried; the passphrase
//! prompt comes after them all.

use std::path::Path;

use zbm_tools::programs;

use crate::config::Options;
use crate::error::LuksError;
use crate::keys::Target;
use crate::keys::gpg::is_gpg_keyfile;
use crate::unlock::Method;
use crate::unlocker::{Key, Unlocker};
use zbm_platform::interaction::VolumeRef;

/// A volume being unlocked: what every key method needs to know.
#[derive(Clone, Copy, Debug)]
pub struct Volume<'v> {
    /// The mapping name.
    pub name: &'v str,
    /// The LUKS device.
    pub device: &'v str,
    /// The LUKS header: the device, or a detached header.
    pub header: &'v str,
    /// The key file specification of the crypttab, or empty.
    pub keyspec: &'v str,
    /// The crypttab options.
    pub options: &'v Options,
    /// The crypttab options as written, for key scripts.
    pub options_text: &'v str,
}

impl<'v> Volume<'v> {
    /// The volume as the key methods of [`crate::keys`] open it.
    fn target(&self) -> Target<'v> {
        Target {
            name: self.name,
            device: self.device,
            args: &self.options.args,
        }
    }
}

/// What a key method did with a volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tried {
    /// It opened the volume.
    Opened,
    /// Another program opened the volume while the method tried.
    Elsewhere,
    /// It does not apply, or its key did not open the volume.
    NotOpened,
}

/// One way of getting the key of a volume and opening it.
pub trait KeyMethod {
    /// The method, as [`crate::unlock::Opened`] reports it.
    fn method(&self) -> Method;

    /// Tries to open `volume`, with the secrets of `session` that earlier
    /// volumes left.
    fn try_open(&self, session: &mut Unlocker<'_>, volume: &Volume<'_>) -> Tried;
}

/// The key methods, in the order they are tried.
pub fn key_methods() -> Vec<Box<dyn KeyMethod>> {
    vec![
        Box::new(KeyScript),
        Box::new(KeyFile),
        Box::new(Tokens),
        Box::new(CachedPassphrases),
        Box::new(YubiKey),
    ]
}

/// A key script of the crypttab prints the key (Debian's crypttab
/// semantics); the key file field is its argument.
pub struct KeyScript;

impl KeyMethod for KeyScript {
    fn method(&self) -> Method {
        Method::KeyScript
    }

    fn try_open(&self, session: &mut Unlocker<'_>, volume: &Volume<'_>) -> Tried {
        let Some(script) = &volume.options.keyscript else {
            return Tried::NotOpened;
        };
        if open_with_keyscript(session, volume, script).is_ok() {
            Tried::Opened
        } else {
            Tried::NotOpened
        }
    }
}

/// Runs a key script with the crypttab variables and opens the volume with
/// what it prints.
fn open_with_keyscript(
    session: &Unlocker<'_>,
    volume: &Volume<'_>,
    script: &str,
) -> Result<(), LuksError> {
    let device = volume.device;
    let executable = session
        .context()
        .files()
        .metadata(Path::new(script))
        .is_ok_and(|metadata| metadata.is_executable());
    if !executable {
        zbm_trace::warn!("key script {script} is not executable");
        return Err(LuksError::KeyScriptNotExecutable {
            script: script.to_owned(),
        });
    }
    zbm_trace::info!("unlocking {device} with key script {script}");
    let key = if volume.keyspec.is_empty() {
        "none"
    } else {
        volume.keyspec
    };
    let mut env = session.exported().to_vec();
    env.extend([
        ("CRYPTTAB_NAME", volume.name),
        ("CRYPTTAB_SOURCE", device),
        ("CRYPTTAB_KEY", key),
        ("CRYPTTAB_OPTIONS", volume.options_text),
        ("CRYPTTAB_TRIED", "0"),
    ]);
    // The script may ask on the console, which it gets for the time.
    let mut printed = None;
    session.with_console(&mut |runner| {
        printed = programs::key_script(runner, script, key, &env)
            .ok()
            .and_then(zbm_platform::Output::into_secret);
    });
    let printed = printed.unwrap_or_default();
    let target = volume.target();
    let opened = session.cryptsetup_open(target.name, device, Key::Stdin(&printed), target.args);
    if opened.is_ok() {
        zbm_trace::info!("unlocked {device} as {} with key script", target.name);
    } else {
        zbm_trace::warn!("key script {script} failed to unlock {device}");
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

    fn try_open(&self, session: &mut Unlocker<'_>, volume: &Volume<'_>) -> Tried {
        let keyspec = volume.keyspec;
        if keyspec.is_empty() || volume.options.keyscript.is_some() {
            return Tried::NotOpened;
        }
        let cached = session
            .gpg_plain
            .get(keyspec)
            .filter(|plain| !plain.is_empty())
            .map(zbm_platform::Secret::duplicate);
        let tried = match cached {
            Some(plain) => open_with_decrypted(session, volume, &plain),
            None => open_with_keyfile(session, volume),
        };
        if tried == Tried::Opened {
            zbm_trace::info!(
                "unlocked {} as {} with key file",
                volume.device,
                volume.name
            );
            Tried::Opened
        } else {
            Tried::NotOpened
        }
    }
}

/// Opens `volume` with a GnuPG key file decrypted for an earlier volume.
fn open_with_decrypted(
    session: &Unlocker<'_>,
    volume: &Volume<'_>,
    plain: &zbm_platform::Secret,
) -> Tried {
    let (device, keyspec) = (volume.device, volume.keyspec);
    zbm_trace::info!("unlocking {device} with GnuPG key file {keyspec} (already decrypted)");
    let target = volume.target();
    let opened = session
        .cryptsetup_open(target.name, device, Key::Stdin(plain), target.args)
        .is_ok();
    outcome(opened, volume)
}

/// [`Tried::Opened`] when `opened`; otherwise the key file failed, which is
/// logged.
fn outcome(opened: bool, volume: &Volume<'_>) -> Tried {
    if opened {
        return Tried::Opened;
    }
    zbm_trace::warn!(
        "key file {} failed to unlock {}",
        volume.keyspec,
        volume.device
    );
    Tried::NotOpened
}

/// Finds the key file of `volume` (mounting its key device if it has one),
/// decrypts it if it is a GnuPG key file, and opens the volume with it.
fn open_with_keyfile(session: &mut Unlocker<'_>, volume: &Volume<'_>) -> Tried {
    let (device, keyspec) = (volume.device, volume.keyspec);
    let named = VolumeRef {
        name: volume.name,
        device,
    };
    let found = session.find_keyfile(keyspec, named);
    let Some(path) = &found.path else {
        session.release_keyfile(&found);
        return Tried::NotOpened;
    };
    let target = volume.target();
    let opened = if volume.options.gpg || is_gpg_keyfile(path, "") {
        match session.gpg_decrypt(path, named) {
            Some(plain) => {
                zbm_trace::info!("unlocking {device} with GnuPG key file {keyspec}");
                let opened = session
                    .cryptsetup_open(target.name, device, Key::Stdin(&plain), target.args)
                    .is_ok();
                // Kept for other volumes until the secrets are forgotten.
                session.gpg_plain.insert(keyspec.to_owned(), plain);
                opened
            }
            None => false,
        }
    } else {
        zbm_trace::info!("unlocking {device} with key file {keyspec}");
        session
            .cryptsetup_open(target.name, device, Key::File(path), target.args)
            .is_ok()
    };
    session.release_keyfile(&found);
    outcome(opened, volume)
}

/// LUKS2 tokens: TPM2, FIDO2 security keys and clevis, unless the crypttab
/// or the configuration turns them off.
pub struct Tokens;

impl KeyMethod for Tokens {
    fn method(&self) -> Method {
        Method::Token
    }

    fn try_open(&self, session: &mut Unlocker<'_>, volume: &Volume<'_>) -> Tried {
        if volume.options.notoken || !session.tokens_enabled() {
            return Tried::NotOpened;
        }
        if session.try_tokens(volume.target(), volume.header).is_ok() {
            return Tried::Opened;
        }
        // Another tool may have unlocked the device meanwhile.
        if session.is_device_open(volume.device) {
            Tried::Elsewhere
        } else {
            Tried::NotOpened
        }
    }
}

/// Passphrases (and YubiKey responses) that opened another volume.
pub struct CachedPassphrases;

impl KeyMethod for CachedPassphrases {
    fn method(&self) -> Method {
        Method::CachedPassphrase
    }

    fn try_open(&self, session: &mut Unlocker<'_>, volume: &Volume<'_>) -> Tried {
        let target = volume.target();
        let opened = session.passphrases.iter().any(|passphrase| {
            session
                .cryptsetup_open(
                    target.name,
                    target.device,
                    Key::Stdin(passphrase),
                    target.args,
                )
                .is_ok()
        });
        if !opened {
            return Tried::NotOpened;
        }
        zbm_trace::info!(
            "unlocked {} as {} with a previously entered passphrase",
            volume.device,
            volume.name
        );
        Tried::Opened
    }
}

/// YubiKey challenge-response, when the crypttab asks for it.
pub struct YubiKey;

impl KeyMethod for YubiKey {
    fn method(&self) -> Method {
        Method::YubiKey
    }

    fn try_open(&self, session: &mut Unlocker<'_>, volume: &Volume<'_>) -> Tried {
        let Some(yubikey) = &volume.options.yubikey else {
            return Tried::NotOpened;
        };
        if session
            .try_ykchalresp(volume.target(), yubikey, volume.options.tries)
            .is_ok()
        {
            Tried::Opened
        } else {
            Tried::NotOpened
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DEFAULT_TRIES;
    use crate::keys::tests::with_session;
    use crate::unlocker::Settings;
    use zbm_platform::Secret;
    use zbm_testkit::machine::{FakeMachine, LuksVolume};

    const DEVICE: &str = "/dev/loop1";

    fn machine() -> FakeMachine {
        FakeMachine::new()
            .with_luks(LuksVolume::new(DEVICE, b"secret"))
            .with_file("/keys/good.key", "secret")
            .with_file("/keys/bad.key", "wrong")
    }

    /// Tries `method` on the volume with the key file `keyspec` and the
    /// crypttab options `text`, after `prepare`.
    fn try_method(
        machine: &FakeMachine,
        method: &dyn KeyMethod,
        keyspec: &str,
        text: &str,
        prepare: impl FnOnce(&mut Unlocker<'_>),
    ) -> Tried {
        let (options, _) = Options::parse(text, DEFAULT_TRIES);
        let volume = Volume {
            name: "v",
            device: DEVICE,
            header: DEVICE,
            keyspec,
            options: &options,
            options_text: text,
        };
        with_session(machine, &Settings::new("/zfsbootmenu"), |session| {
            prepare(session);
            method.try_open(session, &volume)
        })
    }

    #[test]
    fn the_methods_are_tried_in_the_legacy_order() {
        let order: Vec<Method> = key_methods().iter().map(|method| method.method()).collect();
        assert_eq!(
            order,
            [
                Method::KeyScript,
                Method::KeyFile,
                Method::Token,
                Method::CachedPassphrase,
                Method::YubiKey,
            ]
        );
    }

    #[test]
    fn key_files_open_volumes_with_their_contents() {
        let none = |_: &mut Unlocker<'_>| {};
        let good = machine();
        assert_eq!(
            try_method(&good, &KeyFile, "/keys/good.key", "", none),
            Tried::Opened
        );
        let bad = machine();
        assert_eq!(
            try_method(&bad, &KeyFile, "/keys/bad.key", "", none),
            Tried::NotOpened
        );
        assert!(
            bad.kmsg()
                .contains("key file /keys/bad.key failed to unlock /dev/loop1")
        );
        let missing = machine();
        let tried = try_method(&missing, &KeyFile, "/keys/none.key", "", none);
        assert_eq!(tried, Tried::NotOpened);
        // Not with a key script, whose argument the key file is.
        let script = machine();
        let tried = try_method(&script, &KeyFile, "/keys/good.key", "keyscript=/s", none);
        assert_eq!(tried, Tried::NotOpened);
    }

    #[test]
    fn decrypted_gnupg_key_files_are_used_again() {
        let cache = |plain: &'static [u8]| {
            move |session: &mut Unlocker<'_>| {
                let secret = Secret::from_vec(plain.to_vec());
                session.gpg_plain.insert("/keys/k.gpg".to_owned(), secret);
            }
        };
        let good = machine();
        let tried = try_method(&good, &KeyFile, "/keys/k.gpg", "", cache(b"secret"));
        assert_eq!(tried, Tried::Opened);
        assert!(
            good.kmsg()
                .contains("with GnuPG key file /keys/k.gpg (already decrypted)")
        );
        let bad = machine();
        let tried = try_method(&bad, &KeyFile, "/keys/k.gpg", "", cache(b"wrong"));
        assert_eq!(tried, Tried::NotOpened);
    }

    #[test]
    fn yubikeys_are_only_tried_when_asked_for() {
        let none = |_: &mut Unlocker<'_>| {};
        let plain = machine();
        assert_eq!(try_method(&plain, &YubiKey, "", "", none), Tried::NotOpened);
        // No ykchalresp answers on the fake machine.
        let asked = machine();
        let tried = try_method(&asked, &YubiKey, "", "yubikey=2", none);
        assert_eq!(tried, Tried::NotOpened);
    }
}
