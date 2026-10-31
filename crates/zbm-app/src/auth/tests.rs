use std::path::Path;

use pretty_assertions::assert_eq;
use zbm_platform::{Clock as _, Secret};
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// `seconds` since the clock's start.
fn at(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

#[test]
fn an_image_is_locked_when_its_lock_file_exists_even_unreadable() {
    let fake = FakeSystem::new();
    assert_eq!(Lock::of(&fake.files), Lock::Off);

    fake.files
        .file(LOCK_FILE, "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA\n");
    assert_eq!(
        Lock::of(&fake.files),
        Lock::On("$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA".to_owned())
    );
    let other = Path::new("/etc/other-lock");
    fake.files.file(other, "");
    assert!(Lock::at(&fake.files, other).is_on());
}

#[test]
fn without_a_lock_everyone_is_authorised() {
    let authority = Authority::new(Lock::Off);

    assert!(authority.authorized(false, at(0)).is_some());
    assert!(!authority.is_locked(false, at(0)));
    assert!(matches!(
        RecoveryPolicy::of(&authority, at(0)),
        RecoveryPolicy::Open(_)
    ));
}

#[test]
fn a_locked_image_needs_a_proof_which_lasts_while_it_is_used() {
    let authority = Authority::new(Lock::On("hash".to_owned()));
    assert!(authority.is_locked(false, at(0)));
    assert!(authority.authorized(false, at(0)).is_none());
    assert!(matches!(
        RecoveryPolicy::of(&authority, at(0)),
        RecoveryPolicy::Locked
    ));

    authority.prove(Verified::new(), at(10));
    assert!(!authority.is_locked(false, at(10)));
    // Each use keeps it for another while.
    assert!(authority.authorized(false, at(200)).is_some());
    assert!(authority.authorized(false, at(450)).is_some());
    // Unused for longer than that, it is gone, for good.
    assert!(authority.is_locked(false, at(450 + IDLE.as_secs())));
    assert!(
        authority
            .authorized(false, at(450 + IDLE.as_secs()))
            .is_none()
    );
    assert!(authority.authorized(false, at(460)).is_none());

    authority.prove(Verified::new(), at(500));
    authority.forget();
    assert!(authority.authorized(false, at(501)).is_none());
}

#[test]
fn the_gate_runs_the_shell_the_runtime_chose() {
    let fake = FakeSystem::new();
    let authority = Authority::new(Lock::Off);
    let authorized = authority.authorized(false, at(0)).unwrap();

    ShellGate::open(&authorized).run(&fake.runner).unwrap();

    let ran = fake.runner.lines();
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert!(ran[0].ends_with(" -i"), "{ran:?}");
}

#[test]
fn the_recovery_shell_asks_for_the_pin_when_locked_and_locks_again_after() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    let state = State::new(&fake.files, "/zfsbootmenu");
    let open = Authority::new(Lock::Off);
    recovery_shell(&open, &state, &fake.runner, at(0)).unwrap();
    let locked = Authority::new(Lock::On("hash".to_owned()));
    recovery_shell(&locked, &state, &fake.runner, at(0)).unwrap();
    locked.prove(Verified::new(), at(1));
    recovery_shell(&locked, &state, &fake.runner, at(2)).unwrap();
    // The proof went with the shell.
    assert!(locked.is_locked(false, at(3)));
    // A person opened a volume: the shell opens, now and later.
    state.record_unlocked("home", true).unwrap();
    recovery_shell(&locked, &state, &fake.runner, at(4)).unwrap();
    assert!(!locked.is_locked(true, at(4 + 10 * IDLE.as_secs())));

    let ran = fake.runner.lines();
    assert_eq!(ran.len(), 4, "{ran:?}");
    assert!(ran[0].ends_with(" -i"), "{ran:?}");
    assert_eq!(ran[1], "/libexec/zbm lock shell");
    assert!(ran[2].ends_with(" -i"), "{ran:?}");
    assert!(ran[3].ends_with(" -i"), "{ran:?}");
    assert!(matches!(
        RecoveryPolicy::of_image(&Lock::On(String::new())),
        RecoveryPolicy::Locked
    ));
    assert!(matches!(
        RecoveryPolicy::of_image(&Lock::Off),
        RecoveryPolicy::Open(_)
    ));
}

#[test]
fn a_wrong_pin_doubles_the_wait_before_the_next_and_a_right_one_ends_it() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    let state = State::new(&fake.files, "/zfsbootmenu");
    let pin = Secret::copy_from(b"correct horse");
    let lock = Lock::On(zbm_crypt::pin::hash(&pin, b"zbm-test-salt-16").unwrap());
    let wrong = Secret::copy_from(b"not the PIN");

    let started = fake.clock.now();
    for _ in 0..3 {
        assert!(!is_image_pin(&fake.system(), &state, &lock, &wrong));
    }
    // 0 s, then 1 s, then 2 s.
    assert_eq!(fake.clock.now().saturating_sub(started), at(3));
    assert!(is_image_pin(&fake.system(), &state, &lock, &pin));
    assert_eq!(fake.clock.now().saturating_sub(started), at(7));
    assert!(!is_image_pin(&fake.system(), &state, &lock, &wrong));
    assert_eq!(fake.clock.now().saturating_sub(started), at(7));
    assert!(!is_image_pin(&fake.system(), &state, &Lock::Off, &pin));

    assert_eq!(delay(0), at(0));
    assert_eq!(delay(1), at(1));
    assert_eq!(delay(5), at(16));
    assert_eq!(delay(6), LONGEST_DELAY);
    assert_eq!(delay(u32::MAX), LONGEST_DELAY);
}

#[test]
fn a_pin_goes_to_the_keyholder_whose_answer_proves_the_person() {
    let authority = Authority::new(Lock::On("hash".to_owned()));
    for (pin, right) in [("not the PIN", false), ("correct horse", true)] {
        let (client, keyholder) = zbm_sys::socket::Connection::pair().unwrap();
        let checked = std::thread::scope(|scope| {
            let served = zbm_platform::event::spawn_scoped_traced(scope, || {
                let mut heard = Vec::new();
                crate::keyholder::serve(
                    &keyholder,
                    &zbm_testkit::FakeRunner::new(),
                    |_, _| Vec::new(),
                    |pin| {
                        heard = pin.expose().to_vec();
                        pin.expose() == b"correct horse"
                    },
                )
                .unwrap();
                heard
            });
            let pin = Secret::copy_from(pin.as_bytes());
            let verified = check_pin(
                &authority,
                &crate::keyholder::tests::Given::new(client),
                &pin,
                Duration::ZERO,
            );
            (verified.unwrap(), served.join().unwrap())
        });
        assert_eq!(checked, (right, pin.as_bytes().to_vec()));
        assert_eq!(authority.is_locked(false, Duration::ZERO), !right);
    }
    // No keyholder: nothing was checked.
    let pin = Secret::copy_from(b"correct horse");
    assert!(
        check_pin(
            &authority,
            &crate::keyholder::tests::Given::none(),
            &pin,
            Duration::ZERO
        )
        .is_err()
    );
}

#[test]
fn an_empty_pin_or_an_answer_out_of_turn_proves_nothing() {
    use crate::keyholder::FromKeyholder;

    let authority = Authority::new(Lock::On("hash".to_owned()));
    // An empty PIN: no bytes follow, and the keyholder gets an empty one.
    let (client, keyholder) = zbm_sys::socket::Connection::pair().unwrap();
    let checked = std::thread::scope(|scope| {
        let served = zbm_platform::event::spawn_scoped_traced(scope, || {
            let runner = zbm_testkit::FakeRunner::new();
            let mut heard = None;
            crate::keyholder::serve(
                &keyholder,
                &runner,
                |_, _| Vec::new(),
                |pin| {
                    heard = Some(pin.len());
                    false
                },
            )
            .unwrap();
            heard
        });
        let keyholders = crate::keyholder::tests::Given::new(client);
        let verified = check_pin(&authority, &keyholders, &Secret::new(), Duration::ZERO);
        (verified.unwrap(), served.join().unwrap())
    });
    assert_eq!(checked, (false, Some(0)));

    // A keyholder that answers with something else.
    let (client, keyholder) = zbm_sys::socket::Connection::pair().unwrap();
    let keyholders = crate::keyholder::tests::Given::new(client);
    keyholder
        .send(&FromKeyholder::Finished(Vec::new()).encode())
        .unwrap();
    let pin = Secret::copy_from(b"correct horse");
    assert!(check_pin(&authority, &keyholders, &pin, Duration::ZERO).is_err());
    assert!(authority.is_locked(false, Duration::ZERO));
}

#[test]
fn a_wrong_pin_is_refused_even_where_it_cannot_be_counted() {
    // No state directory: the count is not written, the answer stands.
    let fake = FakeSystem::new();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let pin = Secret::copy_from(b"correct horse");
    let lock = Lock::On(zbm_crypt::pin::hash(&pin, b"zbm-test-salt-16").unwrap());

    assert!(!is_image_pin(
        &fake.system(),
        &state,
        &lock,
        &Secret::copy_from(b"wrong pin!")
    ));
    assert!(is_image_pin(&fake.system(), &state, &lock, &pin));
}
