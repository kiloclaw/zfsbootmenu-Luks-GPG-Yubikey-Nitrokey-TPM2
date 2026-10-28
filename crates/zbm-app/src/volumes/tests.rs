use std::path::Path;

use pretty_assertions::assert_eq;
use zbm_luks::device::OpenCheck;
use zbm_luks::unlocker::Settings;
use zbm_platform::KernelLog;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::read::tests::NoPrompts;

/// A machine with `cryptsetup`: /dev/sda2 is open, /dev/sdb2 is locked.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.environment.set("PATH", "/sbin");
    fake.files
        .file_with_mode("/sbin/cryptsetup", "", 0o755)
        .file("/usr/lib/udev/rules.d/95-dm-notify.rules", "")
        .block_device("/dev/sda2")
        .block_device("/dev/sdb2")
        .dir("/fake/holders")
        .file("/fake/holders/sda2", "a1\n");
    fake.runner.respond(
        "blkid -c /dev/null -o device -t TYPE=crypto_LUKS",
        0,
        "/dev/sda2\n/dev/sdb2\n",
    );
    fake.runner
        .respond("cryptsetup luksUUID /dev/sda2", 0, "1111-aaaa\n");
    fake
}

/// The events of `action` on `fake`.
fn act(fake: &FakeSystem, action: &Action) -> Vec<Event> {
    let mut settings = Settings::new("/zfsbootmenu");
    settings.open_check = OpenCheck::Markers("/fake/holders".into());
    let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
    let luks = Luks {
        system: fake.system(),
        settings: &settings,
        interaction: &NoPrompts,
    };
    perform(luks, action).unwrap()
}

/// The last event: how the action ended.
fn ended(events: &[Event]) -> &Outcome {
    match events.last() {
        Some(Event::Finished(outcome)) => outcome,
        other => panic!("no outcome at the end: {other:?}"),
    }
}

#[test]
fn without_cryptsetup_nothing_is_unlocked() {
    let fake = FakeSystem::new();

    let events = act(&fake, &Action::UnlockAll);

    assert_eq!(events.len(), 1);
    assert!(
        matches!(ended(&events), Outcome::Failed { why, .. } if why == &[NO_CRYPTSETUP]),
        "{events:?}"
    );
    let open = Action::Open {
        device: "/dev/sda2".into(),
        name: None,
    };
    assert!(matches!(ended(&act(&fake, &open)), Outcome::Failed { .. }));
}

#[test]
fn an_open_volume_is_reported_open_under_the_name_recorded() {
    let fake = machine();
    fake.files.file(
        "/zfsbootmenu/luks-mappings",
        "luks-1111-aaaa\t/dev/sda2\t\n",
    );
    let open = Action::Open {
        device: "/dev/sda2".into(),
        name: None,
    };

    let events = act(&fake, &open);

    assert_eq!(
        events[0],
        Event::UnlockFinished {
            volume: "luks-1111-aaaa".into(),
            outcome: UnlockOutcome::Opened,
        }
    );
    assert_eq!(
        ended(&events),
        &Outcome::Done("/dev/sda2 is open as luks-1111-aaaa".into())
    );
}

#[test]
fn a_device_that_is_not_there_is_named() {
    let fake = machine();
    let open = Action::Open {
        device: "/dev/none".into(),
        name: Some("x".parse().unwrap()),
    };

    let events = act(&fake, &open);

    assert_eq!(
        ended(&events),
        &Outcome::Failed {
            what: "unable to open /dev/none".into(),
            why: vec!["device '/dev/none' not found".into()],
            next: Vec::new(),
        }
    );
}

#[test]
fn closing_without_mappings_is_done_at_once() {
    let fake = machine();

    assert_eq!(
        ended(&act(&fake, &Action::Close(None))),
        &Outcome::Done("closed the LUKS volumes".into())
    );
    assert_eq!(
        ended(&act(&fake, &Action::Close(Some("a1".parse().unwrap())))),
        &Outcome::Done("closed a1".into())
    );
    let settings = Settings::new("/zfsbootmenu");
    let luks = Luks {
        system: fake.system(),
        settings: &settings,
        interaction: &NoPrompts,
    };
    assert!(perform(luks, &Action::Shell).is_none());
}

#[test]
fn the_causes_of_an_error_are_listed_outermost_first() {
    #[derive(Debug, thiserror::Error)]
    #[error("outer")]
    struct Outer(#[source] std::io::Error);

    let error = Outer(std::io::Error::other("inner"));

    assert_eq!(causes(&error), ["outer", "inner"]);
}

#[test]
fn an_unlocking_run_reports_the_volumes_skipped_and_how_it_ended() {
    let fake = machine();
    fake.files.file("/zfsbootmenu/luks-skipped", "/dev/sda9\n");
    // An empty passphrase skips the volume.
    fake.console.secret(b"");

    let events = act_in_app(&fake, &Action::UnlockAll);

    assert_eq!(
        events[0],
        Event::UnlockFinished {
            volume: "/dev/sdb2".into(),
            outcome: UnlockOutcome::Skipped,
        }
    );
    assert!(
        matches!(ended(&events), Outcome::Done(text) if text.starts_with("unlocked ")),
        "{events:?}"
    );
}

/// A prompter that answers every timed prompt with ESCAPE.
struct Escape;

impl zbm_platform::interaction::Interaction for Escape {
    fn ask(
        &self,
        _: &zbm_platform::interaction::PromptRequest,
    ) -> zbm_platform::interaction::Answer {
        zbm_platform::interaction::Answer::Escape
    }
}

/// The events of `action` on `fake`, through [`crate::App::act`].
fn act_in_app(fake: &FakeSystem, action: &Action) -> Vec<Event> {
    let mut settings = Settings::new("/zfsbootmenu");
    settings.open_check = OpenCheck::Markers("/fake/holders".into());
    let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
    let luks = Luks {
        system: fake.system(),
        settings: &settings,
        interaction: &Escape,
    };
    crate::App::new(zbm_platform::Context::new(fake.system()))
        .with_luks(luks)
        .act(action)
}

#[test]
fn a_typed_passphrase_opens_a_locked_volume_in_an_unlocking_run() {
    let fake = machine();
    fake.console.secret(b"pw");

    let events = act_in_app(&fake, &Action::UnlockAll);

    assert!(
        matches!(
            &events[0],
            Event::UnlockFinished {
                outcome: UnlockOutcome::Opened,
                ..
            }
        ),
        "{events:?}"
    );
    assert!(matches!(ended(&events), Outcome::Done(_)), "{events:?}");
}

#[test]
fn a_volume_that_does_not_open_is_reported_with_its_causes() {
    let fake = machine();
    fake.runner.respond_program("cryptsetup", 2, "");
    let open = Action::Open {
        device: "/dev/sdb2".into(),
        name: Some("b1".parse().unwrap()),
    };

    let events = act_in_app(&fake, &open);

    let failed = matches!(
        &events[0],
        Event::UnlockFinished { volume, outcome: UnlockOutcome::Failed(_) } if volume == "b1"
    );
    assert!(failed, "{events:?}");
    assert!(
        matches!(ended(&events), Outcome::Failed { .. }),
        "{events:?}"
    );
}

#[test]
fn actions_without_a_use_case_end_as_failed() {
    let fake = machine();
    let app = crate::App::new(zbm_platform::Context::new(fake.system()));

    for events in [
        app.act(&Action::UnlockAll),
        act_in_app(&fake, &Action::Shell),
    ] {
        let refused = |what: &String| what == "unable to do that here";
        assert!(
            matches!(ended(&events), Outcome::Failed { what, .. } if refused(what)),
            "{events:?}"
        );
    }
}
