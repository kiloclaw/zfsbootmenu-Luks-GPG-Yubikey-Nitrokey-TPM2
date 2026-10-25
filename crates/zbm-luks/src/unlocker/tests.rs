use super::*;
use crate::testing::{NoPrompts, Prompts, log};
use zbm_platform::clock::Clock;
use zbm_platform::exec::{Output, Status};
use zbm_testkit::fakes::{ConsoleInput, FakeSystem};

#[test]
fn settings_follow_the_legacy_variables() {
    let fake = FakeSystem::new();
    let settings = Settings::from_environment(&fake.environment);
    assert_eq!(settings, Settings::new("/zfsbootmenu"));
    assert_eq!(settings.mappings, Path::new("/zfsbootmenu/luks-mappings"));
    assert!(settings.token_plugins && !settings.retry);
    for (name, value) in [
        ("BASE", "/run/base"),
        ("ZBM_CRYPTTAB", "/tab"),
        ("LUKS_STATE", "/state"),
        ("LUKS_GNUPG_DIR", "/gpg"),
        ("LUKS_TPM2_TCTI", "swtpm:port=1"),
        ("LUKS_NO_TOKEN_PLUGINS", "1"),
        ("LUKS_DEFAULT_TRIES", "08"),
        ("LUKS_RETRY", "1"),
        ("zbm_retry_delay", "7"),
    ] {
        fake.environment.set(name, value);
    }
    let settings = Settings::from_environment(&fake.environment);
    assert_eq!(settings.base, Path::new("/run/base"));
    assert_eq!(settings.skipped, Path::new("/run/base/luks-skipped"));
    assert_eq!(settings.mappings, Path::new("/state"));
    assert_eq!(settings.crypttab, Path::new("/tab"));
    assert_eq!(settings.gnupg_dir, Path::new("/gpg"));
    assert_eq!(settings.tpm2_tcti, "swtpm:port=1");
    assert!(!settings.token_plugins && settings.retry);
    assert_eq!(settings.default_tries, 8);
    assert_eq!(settings.retry_delay, "7");
}

fn with_session(fake: &FakeSystem, body: impl FnOnce(&mut Unlocker<'_>)) {
    let settings = Settings::new("/zfsbootmenu");
    let _log = log(fake);
    let luks = Luks {
        system: fake.system(),
        settings: &settings,
        prompter: &NoPrompts,
    };
    body(&mut Unlocker::new(luks));
}

#[test]
fn token_waits_are_six_digits_at_most() {
    let fake = FakeSystem::new();
    for (value, wait) in [
        (None, 10),
        (Some("0"), 0),
        (Some("08"), 8),
        (Some("010"), 10),
        (Some("999999"), 999_999),
        (Some("1000000"), 10),
        (Some("-5"), 10),
        (Some("+5"), 10),
        (Some("1e3"), 10),
        (Some(""), 10),
    ] {
        if let Some(value) = value {
            fake.files
                .file("/zfsbootmenu/cmdline.d/zbm.luks_token_wait", value);
        }
        with_session(&fake, |session| {
            assert_eq!(session.token_wait(), wait, "{value:?}");
        });
    }
}

#[test]
fn tokens_are_on_unless_turned_off() {
    let fake = FakeSystem::new();
    with_session(&fake, |session| assert!(session.tokens_enabled()));
    for (value, on) in [
        ("0", false),
        ("off", false),
        ("No", false),
        ("", false),
        ("1", true),
    ] {
        fake.files
            .file("/zfsbootmenu/cmdline.d/zbm.luks_tokens", value);
        with_session(&fake, |session| {
            assert_eq!(session.tokens_enabled(), on, "{value:?}");
        });
    }
}

#[test]
fn waiting_ends_with_the_device_or_a_key() {
    let fake = FakeSystem::new();
    fake.console.timeout().timeout();
    let checks = std::cell::Cell::new(0);
    with_session(&fake, |session| {
        let waited = session.wait_for(5, "Insert it", &|| {
            checks.set(checks.get() + 1);
            checks.get() == 3
        });
        assert_eq!(waited, Wait::Present);
    });
    assert_eq!(checks.get(), 3);
    assert_eq!(fake.clock.now(), Duration::from_secs(2));
    assert_eq!(
        fake.console.output_text(),
        "Insert it\nWaiting 5 seconds; press any key to skip.\n"
    );
    assert_eq!(fake.runner.lines(), ["udevadm settle"]);

    // A key skips the wait; the end of the input waits a second each time.
    let fake = FakeSystem::new();
    fake.console.keys(b"x");
    with_session(&fake, |session| {
        assert_eq!(session.wait_for(5, "m", &|| false), Wait::Skipped);
    });
    let fake = FakeSystem::new();
    fake.console.push(ConsoleInput::End);
    with_session(&fake, |session| {
        assert_eq!(session.wait_for(3, "m", &|| false), Wait::TimedOut);
    });
    assert_eq!(fake.clock.now(), Duration::from_secs(3));
    // Without time to wait, only the first check counts.
    let fake = FakeSystem::new();
    with_session(&fake, |session| {
        assert_eq!(session.wait_for(0, "m", &|| false), Wait::TimedOut);
    });
    assert_eq!(fake.console.output_text(), "");
}

#[test]
fn secrets_are_read_with_echo_off() {
    let fake = FakeSystem::new();
    fake.console.secret(b"1234");
    with_session(&fake, |session| {
        let pin = session.read_secret("TPM2 PIN: ").unwrap();
        assert_eq!(pin.expose(), b"1234");
        assert_eq!(session.read_secret("again: "), None);
    });
    assert_eq!(fake.console.echo_changes(), [false, true, false, true]);
    assert_eq!(fake.console.output_text(), "TPM2 PIN: \nagain: \n");
    fake.assert_no_leak(b"1234");
}

#[test]
fn cryptsetup_gets_keys_on_standard_input() {
    let fake = FakeSystem::new();
    fake.runner.on(|command| {
        let stdin_ok =
            matches!(command.get_stdin(), Input::Secret(secret) if secret.expose() == b"pw");
        Some(Ok(if stdin_ok {
            Output::new(Status::exited(0), "")
        } else {
            Output::new(
                Status::exited(2),
                "No key available with this passphrase.\n\n",
            )
        }))
    });
    with_session(&fake, |session| {
        let args = ["--allow-discards".to_owned()];
        let right = Secret::copy_from(b"pw");
        let wrong = Secret::copy_from(b"no");
        let open = |key| session.cryptsetup_open("v", "/dev/sda", key, &[]);
        assert_eq!(
            session.cryptsetup_open("v", "/dev/sda", Key::Stdin(&right), &args),
            Ok(())
        );
        assert_eq!(
            open(Key::Stdin(&wrong)),
            Err(LuksError::Open {
                device: "/dev/sda".to_owned(),
                name: "v".to_owned(),
                status: 2
            })
        );
        assert!(open(Key::File("/k")).is_err());
    });
    assert_eq!(
        fake.runner.lines(),
        [
            "cryptsetup open --type luks --key-file=- --allow-discards /dev/sda v",
            "cryptsetup open --type luks --key-file=- /dev/sda v",
            "cryptsetup open --type luks --key-file=/k /dev/sda v",
        ]
    );
    let missing = FakeSystem::new();
    missing
        .runner
        .on(|_| Some(Err(std::io::Error::other("no cryptsetup"))));
    with_session(&missing, |session| {
        assert!(matches!(
            session.cryptsetup_open("v", "/dev/sda", Key::File("/k"), &[]),
            Err(LuksError::Open { status: 127, .. })
        ));
    });
    assert!(fake.kmsg().contains(
        "|cryptsetup open /dev/sda v failed (2): No key available with this passphrase."
    ));
    assert_eq!(*fake.runner.calls()[2].get_stdin(), Input::Null);
    fake.assert_no_leak(b"pw");
}

#[test]
fn device_mapper_manages_nodes_without_udev_rules() {
    let fake = FakeSystem::new();
    fake.environment.set("PATH", "/sbin");
    with_session(&fake, |session| assert!(!session.available()));
    fake.files.file_with_mode("/sbin/cryptsetup", "", 0o755);
    with_session(&fake, |session| {
        assert!(session.available());
        assert_eq!(session.exported(), [("DM_DISABLE_UDEV", "1")]);
    });
    fake.files
        .file("/usr/lib/udev/rules.d/95-dm-notify.rules", "");
    with_session(&fake, |session| {
        assert!(session.available());
        assert!(session.exported().is_empty());
    });
    assert_eq!(
        fake.kmsg()
            .matches("disabling udev synchronization")
            .count(),
        1
    );
}

#[test]
fn prompts_go_to_the_prompter() {
    let fake = FakeSystem::new();
    let settings = Settings::new("/zfsbootmenu");
    let _log = log(&fake);
    let prompts = Prompts::answering(&[Answer::Continue]);
    let session = Unlocker::new(Luks {
        system: fake.system(),
        settings: &settings,
        prompter: &prompts,
    });
    let request = PromptRequest {
        subject: Subject::Retry {
            name: "root".to_owned(),
            device: "/dev/sda".to_owned(),
        },
        delay: None,
        message: "Unable to unlock LUKS volume /dev/sda".to_owned(),
        on_return: "try again".to_owned(),
        on_escape: "skip this volume".to_owned(),
    };
    assert_eq!(session.timed_prompt(&request), Answer::Continue);
    assert_eq!(session.timed_prompt(&request), Answer::Escape);
    assert_eq!(prompts.messages().len(), 2);
}

#[test]
fn prompts_record_their_legacy_arguments() {
    let request = PromptRequest {
        subject: Subject::Missing("x".to_owned()),
        delay: Some("5".to_owned()),
        message: "LUKS device x is missing".to_owned(),
        on_return: "retry now".to_owned(),
        on_escape: "skip this device".to_owned(),
    };
    assert_eq!(
        request.arguments().join(" "),
        "-d 5 -m LUKS device x is missing -r retry now -e skip this device \
         -p Retrying in %0.2d seconds"
    );
}
