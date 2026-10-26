use pretty_assertions::assert_eq;
use zbm_platform::secret::Secret;

use super::*;

#[test]
fn start_up_runs_stage_by_stage() {
    let mut machine = FakeMachine::new(&DemoOptions::default());

    let mut ticks = 0;
    while machine
        .snapshot()
        .startup
        .iter()
        .any(|s| s.state != StageState::Done)
    {
        machine.tick();
        ticks += 1;
    }

    assert_eq!(ticks, 10);
    assert!(machine.tick().is_empty());
}

#[test]
fn a_locked_volume_stops_start_up_until_it_is_unlocked() {
    let mut machine = FakeMachine::new(&DemoOptions {
        locked: true,
        ..DemoOptions::default()
    });
    let events: Vec<Event> = (0..8).flat_map(|_| machine.tick()).collect();

    let asked = events
        .iter()
        .filter(|e| matches!(e, Event::UnlockRequested { .. }))
        .count();
    let wrong = machine.apply(Action::Unlock {
        volume: "luks-zroot-a".into(),
        method: Method::Tpm2Pin,
        secret: Some(Secret::copy_from(b"0000")),
    });

    assert_eq!(asked, 1);
    assert!(matches!(
        &wrong[..],
        [Event::UnlockRequested {
            tries_left: Some(2),
            notice: Some(Notice::Wrong(_)),
            ..
        }]
    ));
}

#[test]
fn three_wrong_pins_lock_the_tpm_and_offer_the_passphrase() {
    let mut machine = FakeMachine::new(&DemoOptions {
        locked: true,
        ..DemoOptions::default()
    });
    let wrong = |machine: &mut FakeMachine| {
        machine.apply(Action::Unlock {
            volume: "luks-zroot-a".into(),
            method: Method::Tpm2Pin,
            secret: Some(Secret::copy_from(b"9999")),
        })
    };

    wrong(&mut machine);
    wrong(&mut machine);
    let last = wrong(&mut machine);

    assert!(matches!(
        &last[..],
        [Event::UnlockRequested {
            method: Method::Passphrase,
            notice: Some(Notice::Instead(_)),
            ..
        }]
    ));
}

#[test]
fn the_security_key_is_never_found() {
    let mut machine = FakeMachine::new(&DemoOptions {
        locked: true,
        ..DemoOptions::default()
    });

    let events = machine.apply(Action::Unlock {
        volume: "luks-zroot-b".into(),
        method: Method::Fido2,
        secret: None,
    });

    let warned = matches!(
        &events[..],
        [Event::UnlockRequested { notice: Some(Notice::Instead(text)), .. }]
            if text.starts_with("No security key")
    );
    assert!(warned);
}

#[test]
fn skipping_every_volume_skips_the_stage() {
    let mut machine = FakeMachine::new(&DemoOptions {
        locked: true,
        ..DemoOptions::default()
    });
    for volume in ["luks-zroot-a", "luks-zroot-b", "luks-tank"] {
        for _ in 0..6 {
            machine.tick();
        }
        machine.apply(Action::SkipUnlock(volume.into()));
    }
    for _ in 0..3 {
        machine.tick();
    }

    assert_eq!(
        machine.snapshot().startup[data::UNLOCK_STAGE].state,
        StageState::Skipped
    );
}

#[test]
fn copies_and_rollbacks_change_the_storage() {
    let mut machine = FakeMachine::new(&DemoOptions {
        snapshots: 5,
        ..DemoOptions::default()
    });
    let snapshot: zbm_types::SnapshotName = "zroot/ROOT/void@pre-upgrade".parse().unwrap();

    machine.apply(Action::Clone(
        snapshot.clone(),
        "zroot/ROOT/void_NEW".parse().unwrap(),
    ));
    machine.apply(Action::Rollback(snapshot.clone()));
    let names: Vec<String> = machine
        .snapshot()
        .environments
        .iter()
        .map(|e| e.name.to_string())
        .collect();
    let void = &machine.snapshot().environments[0];

    assert!(names.contains(&"zroot/ROOT/void_NEW".to_owned()));
    let target = void.snapshots.iter().find(|s| s.name == snapshot).unwrap();
    assert!(void.snapshots.iter().all(|s| s.created <= target.created));
    assert!(machine.snapshot().pools[0].writable);
}

#[test]
fn leaving_actions_end_the_menu() {
    let mut machine = FakeMachine::new(&DemoOptions::default());

    let events = machine.apply(Action::Boot {
        environment: "tank/ROOT/arch".parse().unwrap(),
        kernel: None,
        command_line: None,
        countdown: false,
    });

    assert_eq!(
        events,
        [Event::Leaving(
            concat!(
                "Booting tank/ROOT/arch with kernel 6.11.5-arch1-1 ",
                "and the command line \"rw quiet splash\""
            )
            .into()
        )]
    );
}
