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
    let request = SecretRequest {
        volume: VOLUME,
        kind: SecretKind::Tpm2Pin,
        attempt: 1,
        tries: None,
        opened_elsewhere: None,
        description: None,
    };
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
