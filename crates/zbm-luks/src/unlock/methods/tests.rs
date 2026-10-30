//! The key methods and their registry: each method's refusals, the order,
//! and every attempt kept.

use super::*;
use crate::config::{DEFAULT_TRIES, Options};
use crate::keys::tests::with_session;
use crate::mappings::Origin;
use crate::unlocker::Settings;
use crate::volume::Entry;
use zbm_crypt::Mapping;
use zbm_platform::Secret;
use zbm_testkit::machine::{FakeMachine, LuksVolume};

const DEVICE: &str = "/dev/loop1";

fn machine() -> FakeMachine {
    FakeMachine::new()
        .with_luks(LuksVolume::new(DEVICE, b"secret"))
        .with_file("/keys/good.key", "secret")
        .with_file("/keys/bad.key", "wrong")
}

/// The volume, closed, with the key file `keyspec` and the crypttab
/// options `text`.
fn volume(keyspec: &str, text: &str) -> Volume<Closed> {
    let (options, _) = Options::parse(text, DEFAULT_TRIES);
    let entry = Entry {
        header: DEVICE.into(),
        keyspec: keyspec.to_owned(),
        options,
        options_text: text.to_owned(),
        origin: Origin::Configured,
    };
    Volume::new(
        Mapping::new(DEVICE.parse().unwrap(), "v".parse().unwrap()),
        entry,
    )
}

/// Tries `method` on [`volume`], after `prepare`.
fn try_method(
    machine: &FakeMachine,
    method: &dyn KeyMethod,
    (keyspec, text): (&str, &str),
    prepare: impl FnOnce(&mut Unlocker<'_>),
) -> Result<String, Refusal> {
    with_session(machine, &Settings::new("/zfsbootmenu"), |session| {
        prepare(session);
        method
            .try_open(session, &volume(keyspec, text))
            .map(|open| open.path())
    })
}

const OPEN: &str = "/dev/mapper/v";

#[test]
fn the_methods_are_tried_in_the_legacy_order() {
    let order: Vec<Method> = Registry::standard().methods().collect();
    assert_eq!(
        order,
        [
            Method::KeyScript,
            Method::KeyFile,
            Method::Token,
            Method::CachedPassphrase,
            Method::YubiKey,
            Method::Passphrase,
        ]
    );
    let alone: Vec<Method> = Registry::key_file().methods().collect();
    assert_eq!(alone, [Method::KeyFile]);
    assert!(format!("{:?}", Registry::key_file()).contains("KeyFile"));
}

#[test]
fn key_files_open_volumes_with_their_contents() {
    let none = |_: &mut Unlocker<'_>| {};
    let good = machine();
    let opened = try_method(&good, &KeyFile, ("/keys/good.key", ""), none);
    assert_eq!(opened.as_deref(), Ok(OPEN));
    let bad = machine();
    // Refused with the reason cryptsetup gave.
    let refused = try_method(&bad, &KeyFile, ("/keys/bad.key", ""), none);
    assert!(matches!(
        refused,
        Err(Refusal::Failed(LuksError::Open { ref why, .. })) if why.contains("no key slot")
    ));
    assert!(
        bad.kmsg()
            .contains("key file /keys/bad.key failed to unlock /dev/loop1")
    );
    let missing = machine();
    let tried = try_method(&missing, &KeyFile, ("/keys/none.key", ""), none);
    assert!(matches!(
        tried,
        Err(Refusal::Failed(LuksError::KeyFile { .. }))
    ));
    // Not with a key script, whose argument the key file is, nor without
    // one.
    let script = machine();
    let tried = try_method(&script, &KeyFile, ("/keys/good.key", "keyscript=/s"), none);
    assert_eq!(tried, Err(Refusal::NotApplicable));
    assert_eq!(
        try_method(&machine(), &KeyFile, ("", ""), none),
        Err(Refusal::NotApplicable)
    );
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
    let tried = try_method(&good, &KeyFile, ("/keys/k.gpg", ""), cache(b"secret"));
    assert_eq!(tried.as_deref(), Ok(OPEN));
    assert!(
        good.kmsg()
            .contains("with GnuPG key file /keys/k.gpg (already decrypted)")
    );
    let bad = machine();
    let tried = try_method(&bad, &KeyFile, ("/keys/k.gpg", ""), cache(b"wrong"));
    assert!(tried.is_err());
}

#[test]
fn passphrases_that_opened_other_volumes_are_tried() {
    let cached = |key: &'static [u8]| {
        move |session: &mut Unlocker<'_>| session.passphrases.push(Secret::copy_from(key))
    };
    let none = |_: &mut Unlocker<'_>| {};
    assert_eq!(
        try_method(&machine(), &CachedPassphrases, ("", ""), none),
        Err(Refusal::NotApplicable)
    );
    let opened = try_method(&machine(), &CachedPassphrases, ("", ""), cached(b"secret"));
    assert_eq!(opened.as_deref(), Ok(OPEN));
    let refused = try_method(&machine(), &CachedPassphrases, ("", ""), cached(b"wrong"));
    assert!(matches!(
        refused,
        Err(Refusal::Failed(LuksError::Open { .. }))
    ));
}

#[test]
fn yubikeys_and_key_scripts_are_only_tried_when_asked_for() {
    let none = |_: &mut Unlocker<'_>| {};
    let plain = machine();
    assert_eq!(
        try_method(&plain, &YubiKey, ("", ""), none),
        Err(Refusal::NotApplicable)
    );
    assert_eq!(
        try_method(&plain, &KeyScript, ("", ""), none),
        Err(Refusal::NotApplicable)
    );
    // No ykchalresp answers on the fake machine, and no key script is
    // there to run.
    let asked = machine();
    let tried = try_method(&asked, &YubiKey, ("", "ykchalresp=2"), none);
    assert!(matches!(
        tried,
        Err(Refusal::Failed(LuksError::YubiKey { .. }))
    ));
    let tried = try_method(&asked, &KeyScript, ("", "keyscript=/missing"), none);
    assert!(matches!(
        tried,
        Err(Refusal::Failed(LuksError::KeyScriptNotExecutable { .. }))
    ));
}

#[test]
fn every_attempt_is_kept_and_the_volume_handed_back() {
    let fake = machine();
    let registry = Registry::of(vec![Rc::new(KeyScript), Rc::new(KeyFile)]);
    let (failed, attempts) = with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
        let not = registry
            .unlock(session, volume("/keys/bad.key", ""))
            .unwrap_err();
        let device = not.volume.device().clone();
        (not, session.attempts(&device).to_vec())
    });
    // The key script did not apply, and is not an attempt.
    assert_eq!(failed.attempts, attempts);
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].method, Method::KeyFile);
    assert_eq!(failed.volume.name().as_str(), "v");
    let error = failed.into_error();
    assert_eq!(error.to_string(), "no key method opened /dev/loop1");
    assert!(matches!(error, LuksError::NotUnlocked { ref attempts, .. } if attempts.len() == 1));

    let good = machine();
    let opened = with_session(&good, &Settings::new("/zfsbootmenu"), |session| {
        let unlocked = registry.unlock(session, volume("/keys/good.key", ""));
        let device: zbm_types::DevicePath = DEVICE.parse().unwrap();
        (unlocked, session.attempts(&device).to_vec())
    });
    match opened {
        (Ok(Unlocked::Opened(volume, Method::KeyFile)), attempts) => {
            assert_eq!(volume.mapping().path(), OPEN);
            assert_eq!(
                attempts,
                [Attempt {
                    method: Method::KeyFile,
                    refusal: None
                }]
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_prompt_ends_the_attempts_with_why() {
    let skipped = NotUnlocked {
        volume: volume("", ""),
        attempts: vec![Attempt {
            method: Method::Passphrase,
            refusal: Some(Refusal::Failed(LuksError::Skipped {
                device: DEVICE.to_owned(),
            })),
        }],
    };
    assert!(matches!(skipped.into_error(), LuksError::Skipped { .. }));
}
