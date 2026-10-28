use std::cell::Cell;

use zbm_app::model::{Notice, Volume, VolumeState};
use zbm_platform::Clock;
use zbm_platform::interaction::{
    Asked, Awaited, SecretKind, SecretRequest, Tell, Told, VolumeRef, WaitRequest,
};
use zbm_platform::interaction::{Interaction, PromptRequest, Subject, Wait};
use zbm_platform::secret::Secret;

use pretty_assertions::assert_eq;

use super::*;

const VOLUME: VolumeRef<'static> = VolumeRef {
    name: "cryptroot",
    device: "/dev/sda2",
};

/// A request for `kind`, attempt `attempt` of 3.
fn request(kind: SecretKind, attempt: u32) -> SecretRequest<'static> {
    SecretRequest {
        volume: VOLUME,
        kind,
        attempt,
        tries: Some(3),
        opened_elsewhere: None,
        description: None,
    }
}

/// Enter in the unlock dialog of `method`, with `secret` typed.
fn unlock(method: Method, secret: &str) -> Action {
    Action::Unlock {
        volume: VOLUME.name.into(),
        method,
        secret: Some(std::sync::Arc::new(Secret::from_vec(
            secret.as_bytes().to_vec(),
        ))),
    }
}

/// Esc in the unlock dialog.
fn skip() -> Action {
    Action::SkipUnlock(VOLUME.name.into())
}

/// The unlock dialog for `method`, showing `notice`.
fn requested(method: Method, tries_left: Option<u32>, notice: Option<Notice>) -> Event {
    Event::UnlockRequested {
        volume: VOLUME.name.into(),
        method,
        tries_left,
        notice,
        wait: None,
    }
}

/// The unlock dialog waiting `left` seconds for what `method` needs.
fn waiting(method: Method, left: u64) -> Event {
    Event::UnlockRequested {
        volume: VOLUME.name.into(),
        method,
        tries_left: None,
        notice: None,
        wait: Some(left),
    }
}

/// What the dialog gave for `request`.
fn ask(front: &MenuFront<'_>, request: &SecretRequest<'_>) -> Asked {
    front.secret(request)
}

/// The secret in `asked`.
fn secret(asked: Asked) -> Vec<u8> {
    match asked {
        Asked::Secret(secret) => secret.expose().to_vec(),
        other => panic!("no secret: {other:?}"),
    }
}

/// The volumes the view lists.
fn volumes(seen: &Seen) -> Vec<Volume> {
    let changed = seen.sent().into_iter().rev().find_map(|event| match event {
        Event::Changed(snapshot) => Some(snapshot.volumes),
        _ => None,
    });
    changed.expect("a changed snapshot")
}

#[test]
fn a_secret_typed_in_the_dialog_comes_back_and_esc_gives_an_empty_one() {
    let fake = FakeSystem::new();
    let (front, seen) = front(&fake);
    seen.reply(Reply::With(unlock(Method::Passphrase, "hunter2")));
    let typed = secret(ask(&front, &request(SecretKind::Passphrase, 1)));
    assert_eq!(typed, b"hunter2");
    assert_eq!(seen.asked(), requested(Method::Passphrase, Some(3), None));
    // The dialog names the volume's device.
    let listed = Volume {
        name: VOLUME.name.into(),
        device: VOLUME.device.into(),
        state: VolumeState::Locked,
        methods: vec![Method::Passphrase],
        pool: None,
        tpm2: None,
    };
    assert_eq!(volumes(&seen), [listed]);

    // Asked again, the passphrase was wrong.
    seen.reply(Reply::With(skip()));
    let typed = secret(ask(&front, &request(SecretKind::Passphrase, 2)));
    assert!(typed.is_empty());
    let wrong = Notice::Wrong("No key available with this passphrase.".into());
    assert_eq!(
        seen.asked(),
        requested(Method::Passphrase, Some(2), Some(wrong))
    );
    assert_eq!(volumes(&seen).len(), 1);

    // Another method chosen in the dialog: this one is not used.
    seen.reply(Reply::With(unlock(Method::Tpm2Pin, "1234")));
    let typed = secret(ask(&front, &request(SecretKind::Passphrase, 1)));
    assert!(typed.is_empty());
}

#[test]
fn another_program_opening_the_volume_or_the_view_failing_ends_the_request() {
    let fake = FakeSystem::new();
    let (front, seen) = front(&fake);
    let checks = Cell::new(0);
    let opened = || {
        checks.set(checks.get() + 1);
        checks.get() == 3
    };
    let mut elsewhere = request(SecretKind::Passphrase, 1);
    elsewhere.opened_elsewhere = Some(&opened);
    seen.reply(Reply::Check);
    assert!(matches!(ask(&front, &elsewhere), Asked::Elsewhere));
    // Looked at once a second.
    assert_eq!(checks.get(), 3);
    assert_eq!(fake.clock.now(), Duration::from_secs(2));

    seen.reply(Reply::Fail);
    let passphrase = request(SecretKind::Passphrase, 1);
    assert!(matches!(ask(&front, &passphrase), Asked::Ended));
    assert!(matches!(ask(&front, &passphrase), Asked::Ended));
    assert_eq!(seen.questions(), 2);
}

#[test]
fn each_secret_is_asked_for_with_its_method() {
    let fake = FakeSystem::new();
    let (front, seen) = front(&fake);
    let kinds = [
        (SecretKind::Tpm2Pin, Method::Tpm2Pin),
        (SecretKind::Fido2Pin, Method::Fido2Pin),
        (
            SecretKind::YubiKeyPassphrase("2".into()),
            Method::YubiKeyPassphrase,
        ),
        (
            SecretKind::KeyFilePassphrase("/key.gpg".into()),
            Method::GnupgKeyFile,
        ),
        (
            SecretKind::SecretKeyPassphrase("/key.gpg".into()),
            Method::GnupgKeyFile,
        ),
        (
            SecretKind::CardPin {
                serial: "D276".into(),
                left: "3".into(),
            },
            Method::OpenPgpCardPin,
        ),
    ];
    for (kind, method) in kinds {
        seen.reply(Reply::With(unlock(method, "1234")));
        assert_eq!(secret(ask(&front, &request(kind, 1))), b"1234");
        assert_eq!(seen.asked(), requested(method, Some(3), None));
    }
}

#[test]
fn what_failed_is_said_by_the_next_dialog_and_what_takes_a_while_meanwhile() {
    let fake = FakeSystem::new();
    let (front, seen) = front(&fake);
    let refused = |text| Tell::new(Told::Refused, text);
    front.notify(refused("The TPM refused."));
    seen.reply(Reply::With(skip()));
    let _ = ask(&front, &request(SecretKind::Tpm2Pin, 1));
    let instead = Notice::Instead("The TPM refused.".into());
    assert_eq!(
        seen.asked(),
        requested(Method::Tpm2Pin, Some(3), Some(instead))
    );

    front.notify(refused("Wrong PIN."));
    front.notify(refused("2 tries left."));
    seen.reply(Reply::With(skip()));
    let _ = ask(&front, &request(SecretKind::Tpm2Pin, 2));
    let wrong = Notice::Wrong("Wrong PIN. 2 tries left.".into());
    assert_eq!(
        seen.asked(),
        requested(Method::Tpm2Pin, Some(2), Some(wrong))
    );

    for told in [Told::Working, Told::Touch, Told::Heading] {
        let text = "Unlocking /dev/sda2 ...";
        front.notify(Tell::new(told, text));
        assert_eq!(seen.sent().last(), Some(&Event::Progress(text.into())));
    }
}

#[test]
fn the_timed_prompts_are_questions() {
    let fake = FakeSystem::new();
    let (front, seen) = front(&fake);
    let mut prompt = PromptRequest {
        subject: Subject::Retry {
            name: VOLUME.name.into(),
            device: VOLUME.device.into(),
        },
        delay: zbm_types::RetryDelay::new(10),
        message: "Unlocking failed".into(),
        on_return: "retry".into(),
        on_escape: "skip".into(),
    };
    seen.reply(Reply::With(Action::Answer(Answer::Escape)));
    assert_eq!(front.ask(&prompt), Answer::Escape);
    let retry = Question::RetryUnlock {
        volume: VOLUME.name.into(),
    };
    assert_eq!(seen.asked(), asked(retry, Some(10)));

    prompt.subject = Subject::Missing("UUID=1234".into());
    prompt.delay = None;
    seen.reply(Reply::With(Action::Answer(Answer::Continue)));
    assert_eq!(front.ask(&prompt), Answer::Continue);
    let missing = Question::MissingVolume {
        device: "UUID=1234".into(),
    };
    assert_eq!(seen.asked(), asked(missing, Some(30)));
}

#[test]
fn a_device_is_waited_for_in_the_dialog_with_the_time_left() {
    let fake = FakeSystem::new();
    let (front, seen) = front(&fake);
    let present = Cell::new(true);
    let is_present: &dyn Fn() -> bool = &|| present.get();
    let request = |awaited| WaitRequest {
        volume: VOLUME,
        awaited,
        seconds: 10,
        present: is_present,
    };
    let wait = |awaited| front.wait(&request(awaited));
    assert_eq!(wait(Awaited::YubiKey), Wait::Present);
    assert_eq!(seen.questions(), 0);

    present.set(false);
    seen.reply(Reply::With(skip()));
    assert_eq!(wait(Awaited::SecurityKey), Wait::Skipped);
    assert_eq!(seen.asked(), waiting(Method::Fido2, 10));

    // Enter looks again; time passes meanwhile.
    seen.reply(Reply::Late(4, unlock(Method::OpenPgpCard, "")))
        .reply(Reply::With(Action::Answer(Answer::Continue)));
    assert_eq!(wait(Awaited::OpenPgpKey), Wait::Present);
    assert_eq!(seen.asked(), waiting(Method::OpenPgpCard, 6));

    seen.reply(Reply::Late(10, unlock(Method::KeyDevice, "")));
    let device = Awaited::KeyDevice {
        spec: "LABEL=keys".into(),
        file: "/key".into(),
    };
    assert_eq!(wait(device.clone()), Wait::TimedOut);
    assert_eq!(seen.asked(), waiting(Method::KeyDevice, 10));

    // The device came while the dialog waited.
    let since = fake.clock.now();
    let comes = || fake.clock.now() >= since + Duration::from_secs(3);
    let arrives = WaitRequest {
        present: &comes,
        ..request(Awaited::YubiKey)
    };
    seen.reply(Reply::Check);
    assert_eq!(front.wait(&arrives), Wait::Present);
    // It came while Enter looked again.
    seen.reply(Reply::Late(3, unlock(Method::YubiKey, "")));
    let since = fake.clock.now();
    let comes = || fake.clock.now() >= since + Duration::from_secs(3);
    let arrives = WaitRequest {
        present: &comes,
        ..request(Awaited::YubiKey)
    };
    assert_eq!(front.wait(&arrives), Wait::Present);

    seen.reply(Reply::Fail);
    assert_eq!(wait(device), Wait::TimedOut);
}

#[test]
fn a_program_that_asks_on_the_console_gets_it_and_the_whole_system() {
    let fake = FakeSystem::new();
    let (front, seen) = front(&fake);
    let mut given = false;
    Interaction::lease_console(&front, &mut |system| given = system.is_some());
    assert!(given);
    assert_eq!(seen.suspended(), 1);
}
