//! The keyholder's messages on the wire, read back as they were sent.

use pretty_assertions::assert_eq;

use super::*;

/// The keyholder's requests for each secret and each wait.
fn requests() -> Vec<FromKeyholder> {
    let secret = |kind| {
        FromKeyholder::Secret(SecretAsk {
            name: "root".to_owned(),
            device: "/dev/sda2".to_owned(),
            kind,
            attempt: 2,
            tries: Some(3),
            description: Some("/dev/sda2 (data, 8 GiB)".to_owned()),
            may_open_elsewhere: true,
        })
    };
    let wait = |awaited| {
        FromKeyholder::Wait(WaitAsk {
            name: "root".to_owned(),
            device: "/dev/sda2".to_owned(),
            awaited,
            seconds: 30,
        })
    };
    let mut from = Vec::new();
    from.extend(
        [
            SecretKind::Passphrase,
            SecretKind::HexKey,
            SecretKind::RawKey,
            SecretKind::Tpm2Pin,
            SecretKind::Fido2Pin,
            SecretKind::YubiKeyPassphrase("2".to_owned()),
            SecretKind::KeyFilePassphrase("/k.gpg".to_owned()),
            SecretKind::SecretKeyPassphrase("0xAB".to_owned()),
            SecretKind::CardPin {
                serial: "D276".to_owned(),
                left: "3".to_owned(),
            },
        ]
        .map(secret),
    );
    from.extend(
        [
            Awaited::SecurityKey,
            Awaited::OpenPgpKey,
            Awaited::YubiKey,
            Awaited::KeyDevice {
                spec: "LABEL=keys".to_owned(),
                file: "/root.key".to_owned(),
            },
        ]
        .map(wait),
    );
    from
}

#[test]
fn every_message_of_the_keyholder_reads_back_as_it_was_sent() {
    let mut from = vec![
        FromKeyholder::Ask(PromptRequest {
            subject: Subject::Retry {
                name: "root".to_owned(),
                device: "/dev/sda2".to_owned(),
            },
            delay: RetryDelay::new(7),
            message: "not opened".to_owned(),
            on_return: "retry".to_owned(),
            on_escape: "skip".to_owned(),
        }),
        FromKeyholder::Ask(PromptRequest {
            subject: Subject::Missing("LABEL=keys".to_owned()),
            delay: None,
            message: "missing".to_owned(),
            on_return: "look again".to_owned(),
            on_escape: "skip".to_owned(),
        }),
        FromKeyholder::Ask(PromptRequest {
            subject: Subject::LoadFailed {
                kernel: "/mnt/boot/vmlinuz".to_owned(),
                initramfs: "/mnt/boot/initramfs.img".to_owned(),
            },
            delay: RetryDelay::new(10),
            message: "Unable to load kernel or initramfs into memory".to_owned(),
            on_return: String::new(),
            on_escape: String::new(),
        }),
        FromKeyholder::Ask(PromptRequest {
            subject: Subject::KexecFailed {
                kernel: "/boot/vmlinuz".to_owned(),
            },
            delay: RetryDelay::new(10),
            message: "kexec run of /boot/vmlinuz failed!".to_owned(),
            on_return: String::new(),
            on_escape: String::new(),
        }),
        FromKeyholder::Lease,
        FromKeyholder::Returned,
        FromKeyholder::Answered(true),
        FromKeyholder::Finished(vec![
            Event::UnlockFinished {
                volume: "a".to_owned(),
                outcome: UnlockOutcome::Opened,
            },
            Event::UnlockFinished {
                volume: "b".to_owned(),
                outcome: UnlockOutcome::Skipped,
            },
            Event::UnlockFinished {
                volume: "c".to_owned(),
                outcome: UnlockOutcome::Failed("wrong key".to_owned()),
            },
            Event::Finished(Outcome::Done("unlocked".to_owned())),
        ]),
    ];
    from.extend(requests());
    from.extend(
        [
            Told::Working,
            Told::Touch,
            Told::Refused,
            Told::Heading,
            Told::Booting,
        ]
        .map(|told| {
            FromKeyholder::Notify(Notice {
                told,
                text: "Unlocking /dev/sda2".to_owned(),
                highlight: Some("/dev/sda2".to_owned()),
            })
        }),
    );
    let outcomes = [
        Outcome::Warning("note".to_owned()),
        Outcome::Cancelled,
        Outcome::Failed {
            what: "unable to unlock".to_owned(),
            why: vec!["no key".to_owned()],
            next: vec!["zluks unlock".to_owned()],
        },
    ];
    from.extend(outcomes.map(|outcome| FromKeyholder::Finished(vec![Event::Finished(outcome)])));
    for message in from {
        assert_eq!(FromKeyholder::decode(&message.encode()), Ok(message));
    }
}
