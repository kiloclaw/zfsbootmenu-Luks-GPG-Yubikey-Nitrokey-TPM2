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
        subject: VOLUME.into(),
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

#[test]
fn the_resume_question_clears_warns_and_reads_the_answer_typed() {
    let cases = [
        ("NORESUME", Resume::Noresume),
        ("  DANGEROUS\t", Resume::Dangerous),
        ("noresume", Resume::Refuse),
        ("", Resume::Refuse),
    ];
    for (typed, answer) in cases {
        let fake = FakeSystem::new();
        fake.console.line(typed);
        let request = ResumeRequest { pool: "tank" };
        assert_eq!(console(&fake).resume(&request), answer, "{typed:?}");
        assert_eq!(
            fake.console.screens()[..2],
            [Screen::Clear, Screen::ShowCursor]
        );
        let shown = fake.console.output_text();
        assert!(shown.starts_with("WARNING!!!\n"), "{shown}");
        assert!(shown.contains("\n    tank\n"), "{shown}");
        assert!(shown.ends_with("Proceed [No] ?\n> "), "{shown}");
    }

    // The input ended: nothing was typed.
    let fake = FakeSystem::new();
    fake.console.push(ConsoleInput::End);
    let request = ResumeRequest { pool: "tank" };
    assert_eq!(console(&fake).resume(&request), Resume::Refuse);
}

/// A request for the key of the encryption root `root`, attempt `attempt`.
fn zfs_key(root: &str, kind: SecretKind, attempt: u32) -> SecretRequest<'_> {
    SecretRequest {
        subject: SecretSubject::EncryptionRoot(root),
        kind,
        attempt,
        tries: Some(3),
        opened_elsewhere: None,
        description: None,
    }
}

#[test]
fn a_zfs_key_is_asked_as_zfs_asks_it_on_a_clear_screen() {
    let fake = FakeSystem::new();
    fake.console.secret(b"wrong").secret(b"right");
    let console = console(&fake);

    let first = console.secret(&zfs_key("zroot/ROOT", SecretKind::Passphrase, 1));
    assert_eq!(
        fake.console.screens()[..2],
        [Screen::Clear, Screen::MoveTo { row: 0, column: 0 }]
    );
    let cleared = fake.console.screen_count(Screen::Clear);
    let second = console.secret(&zfs_key("zroot/ROOT", SecretKind::Passphrase, 2));

    assert_eq!(first.secret().unwrap().expose(), b"wrong");
    assert_eq!(second.secret().unwrap().expose(), b"right");
    // The second attempt follows zfs's message on the same screen.
    assert_eq!(fake.console.screen_count(Screen::Clear), cleared);
    assert_eq!(
        fake.console.output_text(),
        "Enter passphrase for 'zroot/ROOT':\nEnter passphrase for 'zroot/ROOT':\n"
    );
    assert_eq!(fake.console.echo_changes(), [false, true, false, true]);

    let fake = FakeSystem::new();
    fake.console.push(ConsoleInput::End);
    let ended = console_of(&fake).secret(&zfs_key("tank", SecretKind::HexKey, 1));
    assert!(matches!(ended, Asked::Ended));
    assert_eq!(fake.console.output_text(), "Enter hex key for 'tank':\n");
}

/// The plain console of `fake`, in colour.
fn console_of(fake: &FakeSystem) -> ConsoleInteraction<'_> {
    ConsoleInteraction::new(fake.system())
}

#[test]
fn a_boot_is_said_in_the_legacy_colours() {
    let fake = FakeSystem::new();
    console_of(&fake).booting("/boot/vmlinuz-6.1", "zroot/ROOT/void");
    assert_eq!(
        fake.console.output_text(),
        "\nBooting \x1b[1;33m/boot/vmlinuz-6.1\x1b[0m for \x1b[0;36mzroot/ROOT/void\x1b[0m ...\n\n"
    );

    // Told as a message, the kernel is highlighted.
    let fake = FakeSystem::new();
    console_of(&fake).notify(Tell {
        told: Told::Booting,
        text: "Booting /boot/vmlinuz-6.1 for zroot/ROOT/void ...",
        highlight: Some("/boot/vmlinuz-6.1"),
    });
    assert_eq!(
        fake.console.output_text(),
        "\nBooting \x1b[1;33m/boot/vmlinuz-6.1\x1b[0m for zroot/ROOT/void ...\n\n"
    );
    // Without a kernel to highlight, the line as it is.
    let fake = FakeSystem::new();
    console_of(&fake).notify(Tell::new(Told::Booting, "Booting for zroot ..."));
    assert_eq!(fake.console.output_text(), "\nBooting for zroot ...\n\n");
}

/// What `prompt` shows on a fresh console: its text and screen changes.
fn shown(run: impl FnOnce(&FakeSystem)) -> (String, Vec<Screen>) {
    let fake = FakeSystem::new();
    fake.console.keys(b"\r");
    run(&fake);
    (fake.console.output_text(), fake.console.screens())
}

#[test]
fn a_failed_boot_shows_the_legacy_prompts() {
    let kexec = PromptRequest {
        subject: Subject::KexecFailed {
            kernel: "/boot/vmlinuz-6.1".to_owned(),
        },
        delay: zbm_types::RetryDelay::new(10),
        message: "kexec run of /boot/vmlinuz-6.1 failed!".to_owned(),
        on_return: String::new(),
        on_escape: String::new(),
    };
    let asked = shown(|fake| {
        assert_eq!(console_of(fake).ask(&kexec), Answer::Continue);
    });
    let legacy = shown(|fake| {
        let _ = TimedPrompt::new()
            .delay(10)
            .message(colorize("red", "kexec run of /boot/vmlinuz-6.1 failed!"))
            .run(&fake.system());
    });
    assert_eq!(asked, legacy);
    assert!(!asked.1.contains(&Screen::ShowCursor), "{:?}", asked.1);

    let load = PromptRequest {
        subject: Subject::LoadFailed {
            kernel: "/mnt/boot/vmlinuz".to_owned(),
            initramfs: "/mnt/boot/initramfs.img".to_owned(),
        },
        message: "Unable to load kernel or initramfs into memory".to_owned(),
        ..kexec
    };
    let asked = shown(|fake| {
        assert_eq!(console_of(fake).ask(&load), Answer::Continue);
    });
    let legacy = shown(|fake| {
        let _ = TimedPrompt::new()
            .delay(10)
            .message(colorize(
                "red",
                "Unable to load kernel or initramfs into memory",
            ))
            .message(colorize("orange", "/mnt/boot/vmlinuz"))
            .message(colorize("orange", "/mnt/boot/initramfs.img"))
            .run(&fake.system());
    });
    assert_eq!(asked, legacy);
}

#[test]
fn the_countdown_keeps_the_screen_before_a_zfs_key() {
    let fake = FakeSystem::new();
    fake.console.secret(b"key");
    let console = console(&fake).clears_for_keys(false);
    let asked = console.secret(&zfs_key("zroot/ROOT", SecretKind::Passphrase, 1));
    assert_eq!(asked.secret().unwrap().expose(), b"key");
    assert_eq!(fake.console.screen_count(Screen::Clear), 0);
    assert_eq!(
        fake.console.output_text(),
        "Enter passphrase for 'zroot/ROOT':\n"
    );
}

#[test]
fn a_chroot_s_prompt_counts_down_to_entering_it_as_zfs_chroot_s_did() {
    let chroot = PromptRequest {
        subject: Subject::Chroot {
            filesystem: "tank/ROOT/be".parse().unwrap(),
        },
        delay: zbm_types::RetryDelay::new(10),
        message: String::new(),
        on_return: "Enter r/o chroot".to_owned(),
        on_escape: "Enter r/w chroot".to_owned(),
    };
    let asked = shown(|fake| {
        assert_eq!(console_of(fake).ask(&chroot), Answer::Continue);
    });
    let legacy = shown(|fake| {
        let _ = TimedPrompt::new()
            .delay(10)
            .on_escape("Enter r/w chroot")
            .on_return("Enter r/o chroot")
            .prompt(|seconds| {
                format!(
                    "Entering chroot in {} seconds",
                    colorize("yellow", &format!("{seconds:02}"))
                )
            })
            .run(&fake.system());
    });
    assert_eq!(asked, legacy);
    assert!(asked.0.contains("Entering chroot in"), "{}", asked.0);
    // The chroot clears the screen itself, once it knows the mode.
    assert!(!asked.1.contains(&Screen::ShowCursor), "{:?}", asked.1);

    let fake = FakeSystem::new();
    fake.console.keys(b"\x1b");
    assert_eq!(console_of(&fake).ask(&chroot), Answer::Escape);
}
