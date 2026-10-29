use std::cell::Cell;

use zbm_platform::clock::Clock;
use zbm_platform::interaction::VolumeRef;
use zbm_testkit::fakes::{ConsoleInput, FakeSystem};

use super::*;

/// The volume the tests ask about.
const VOLUME: VolumeRef<'static> = VolumeRef {
    name: "luks-1",
    device: "/dev/vda2",
};

/// The plain console of `fake`.
fn console(fake: &FakeSystem) -> ConsoleInteraction<'_> {
    ConsoleInteraction::new(fake.system()).colors(false)
}

/// A request for a secret of `kind`, attempt `attempt`.
fn secret(kind: SecretKind, attempt: u32) -> SecretRequest<'static> {
    SecretRequest {
        volume: VOLUME,
        kind,
        attempt,
        tries: None,
        opened_elsewhere: None,
        description: None,
    }
}

/// A wait of `seconds` for a YubiKey that is there when `present` says so.
fn yubikey(seconds: u64, present: &dyn Fn() -> bool) -> WaitRequest<'_> {
    WaitRequest {
        volume: VOLUME,
        awaited: Awaited::YubiKey,
        seconds,
        present,
    }
}

#[test]
fn waiting_ends_with_the_device_or_a_key() {
    let fake = FakeSystem::new();
    fake.console.timeout().timeout();
    let checks = Cell::new(0);
    let present = || {
        checks.set(checks.get() + 1);
        checks.get() == 3
    };
    assert_eq!(console(&fake).wait(&yubikey(5, &present)), Wait::Present);
    assert_eq!(checks.get(), 3);
    assert_eq!(fake.clock.now(), Duration::from_secs(2));
    assert_eq!(
        fake.console.output_text(),
        "Insert the YubiKey for /dev/vda2\nWaiting 5 seconds; press any key to skip.\n"
    );

    // A key skips the wait; the end of the input waits a second each time.
    let fake = FakeSystem::new();
    fake.console.keys(b"x");
    assert_eq!(console(&fake).wait(&yubikey(5, &|| false)), Wait::Skipped);
    let fake = FakeSystem::new();
    fake.console.push(ConsoleInput::End);
    assert_eq!(console(&fake).wait(&yubikey(3, &|| false)), Wait::TimedOut);
    assert_eq!(fake.clock.now(), Duration::from_secs(3));
    // Without time to wait, only the first check counts.
    let fake = FakeSystem::new();
    assert_eq!(console(&fake).wait(&yubikey(0, &|| false)), Wait::TimedOut);
    assert_eq!(fake.console.output_text(), "");
}

#[test]
fn secrets_are_read_with_echo_off() {
    let fake = FakeSystem::new();
    fake.console.secret(b"1234");
    let request = secret(SecretKind::Tpm2Pin, 1);
    let interaction = console(&fake);
    let Asked::Secret(pin) = interaction.secret(&request) else {
        panic!("no PIN");
    };
    assert_eq!(pin.expose(), b"1234");
    assert!(matches!(interaction.secret(&request), Asked::Ended));
    assert_eq!(fake.console.echo_changes(), [false, true, false, true]);
    let asked = "\nUnlock /dev/vda2 with the TPM\n\
                 TPM2 PIN (leave empty to use another method): \n";
    assert_eq!(fake.console.output_text(), asked.repeat(2));
    fake.assert_no_leak(b"1234");
}

#[test]
fn messages_are_painted_by_what_they_tell() {
    let fake = FakeSystem::new();
    let interaction = ConsoleInteraction::new(fake.system());
    interaction.notify(Tell::new(Told::Working, "Unlocking /dev/vda2 ..."));
    interaction.notify(Tell::new(Told::Touch, "Touch the key"));
    interaction.notify(Tell::new(Told::Refused, "Card blocked"));
    interaction.notify(Tell {
        told: Told::Heading,
        text: "Unlock /dev/vda2 with FIDO2",
        highlight: Some("/dev/vda2"),
    });
    let expected = format!(
        "Unlocking /dev/vda2 ...\n{}{}\n{}",
        colorize("yellow", "Touch the key\n"),
        colorize("red", "Card blocked\n"),
        colorize(
            "cyan",
            &format!("Unlock {} with FIDO2\n", colorize("yellow", "/dev/vda2"))
        ),
    );
    assert_eq!(fake.console.output_text(), expected);
}

#[test]
fn the_passphrase_prompt_names_the_volume_and_notices_an_unlock_elsewhere() {
    let fake = FakeSystem::new();
    fake.console.timeout().secret(b"pw");
    let mut request = secret(SecretKind::Passphrase, 2);
    request.description = Some("/dev/vda2 (root, 20G)");
    let Asked::Secret(passphrase) = console(&fake).secret(&request) else {
        panic!("no passphrase");
    };
    assert_eq!(passphrase.expose(), b"pw");
    assert_eq!(
        fake.console.output_text(),
        "\nUnlock LUKS volume /dev/vda2 (root, 20G)\n\
         No key available with this passphrase.\n\
         Passphrase (leave empty to skip): \n"
    );
    assert_eq!(fake.console.echo_changes(), [false, true]);

    // Opened over SSH meanwhile: the prompt goes.
    let fake = FakeSystem::new();
    fake.console.timeout();
    let request = SecretRequest {
        opened_elsewhere: Some(&|| true),
        ..secret(SecretKind::Passphrase, 1)
    };
    assert!(matches!(console(&fake).secret(&request), Asked::Elsewhere));
    assert!(
        fake.console
            .output_text()
            .contains("Unlock LUKS volume /dev/vda2\n")
    );
}

#[test]
fn each_secret_has_its_heading_and_prompt() {
    let card = SecretKind::CardPin {
        serial: "000F1234".to_owned(),
        left: "3".to_owned(),
    };
    let (heading, prompt) = secret_prompt(&card, "/dev/vda2").unwrap();
    assert_eq!(
        heading.as_deref(),
        Some("Unlock /dev/vda2 with OpenPGP card 000F1234")
    );
    assert_eq!(
        prompt,
        "Card PIN (3 attempts left; leave empty to use another method): "
    );
    assert!(secret_prompt(&SecretKind::Passphrase, "/dev/vda2").is_none());
}

#[test]
fn a_wait_says_what_it_waits_for() {
    let fake = FakeSystem::new();
    let request = WaitRequest {
        awaited: Awaited::OpenPgpKey,
        ..yubikey(1, &|| false)
    };
    fake.console.timeout();
    assert_eq!(console(&fake).wait(&request), Wait::TimedOut);
    assert!(
        fake.console.output_text().starts_with(
            "Insert the OpenPGP security key (YubiKey, Nitrokey, ...) for /dev/vda2\n"
        )
    );
}

#[test]
fn a_retry_prompt_leaves_the_screen_clear() {
    let fake = FakeSystem::new();
    fake.console.keys(b"\r");
    let request = PromptRequest {
        subject: Subject::Retry {
            name: "luks-1".to_owned(),
            device: "/dev/vda2".to_owned(),
        },
        delay: None,
        message: "Unable to unlock /dev/vda2".to_owned(),
        on_return: "try again".to_owned(),
        on_escape: "skip it".to_owned(),
    };
    assert_eq!(console(&fake).ask(&request), Answer::Continue);
    let screens = fake.console.screens();
    assert!(
        screens.ends_with(&[Screen::ShowCursor, Screen::Clear]),
        "{screens:?}"
    );
}

#[test]
fn a_heading_without_a_device_is_plain() {
    let fake = FakeSystem::new();
    console(&fake).notify(Tell {
        told: Told::Heading,
        text: "Unlocking the volumes",
        highlight: None,
    });
    assert_eq!(fake.console.output_text(), "\nUnlocking the volumes\n");
}
