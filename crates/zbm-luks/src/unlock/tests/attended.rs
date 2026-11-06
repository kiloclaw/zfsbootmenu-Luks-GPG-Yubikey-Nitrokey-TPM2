//! Whether a person opened a volume (ROADMAP.md, SE10).

use super::*;

#[test]
fn a_person_opened_a_volume_with_a_typed_secret_or_an_interactive_token() {
    let fake = machine(&[A], &[]);
    let prompts = Prompts::new(&fake);
    with_session(&fake, &settings(), &prompts, |session| {
        let (silent, asked) = (device("/dev/loop1"), device("/dev/loop2"));
        session.silently_opened.insert(silent.to_string());
        assert!(!session.is_attended(Method::Token, &silent));
        assert!(session.is_attended(Method::Token, &asked));
        for typed in [
            Method::Passphrase,
            Method::CachedPassphrase,
            Method::YubiKey,
        ] {
            assert!(session.is_attended(typed, &silent), "{typed:?}");
        }
        for unattended in [Method::KeyFile, Method::KeyScript] {
            assert!(!session.is_attended(unattended, &asked), "{unattended:?}");
        }
    });
}

#[test]
fn a_passphrase_typed_over_ssh_is_recorded_as_remote_not_attended() {
    let fake = machine(&[A], &[]);
    fake.console.secret(b"secret1");
    let prompts = Prompts::new(&fake);
    let mut remote = settings();
    remote.presence = crate::unlocker::Presence::Remote;

    let summary = with_session(&fake, &remote, &prompts, unlock_all);

    assert!(matches!(summary, Ok(UnlockSummary { unlocked: 1, .. })));
    assert_eq!(unlocked(&fake), format!("luks-{}\tremote\n", A.uuid));
}
