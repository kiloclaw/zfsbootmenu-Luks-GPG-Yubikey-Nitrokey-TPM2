use std::sync::Arc;

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
        secret: Some(Arc::new(Secret::copy_from(b"0000"))),
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
            secret: Some(Arc::new(Secret::copy_from(b"9999"))),
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
fn an_import_toggles_the_access_rewinds_and_imports_a_foreign_pool() {
    let mut machine = FakeMachine::new(&DemoOptions {
        foreign: true,
        ..DemoOptions::default()
    });
    let import = |pool: &str, access, policy, rewind| {
        let options = ImportOptions {
            access,
            policy,
            rewind,
            id: None,
        };
        Action::Import(pool.parse().unwrap(), options)
    };
    let strict = zbm_types::ImportPolicy::Strict;
    let said = |events: Vec<Event>| match events.last() {
        Some(Event::Finished(Outcome::Done(text) | Outcome::Warning(text))) => text.clone(),
        other => panic!("not finished: {other:?}"),
    };
    let pool = |machine: &FakeMachine, name: &str| {
        let pools = &machine.snapshot().pools;
        pools
            .iter()
            .find(|p| p.name.as_str() == name)
            .cloned()
            .unwrap()
    };

    let events = machine.apply(import("zroot", Access::ReadWrite, strict, false));
    assert_eq!(said(events), "zroot imported read/write");
    let events = machine.apply(import("zroot", Access::ReadWrite, strict, false));
    assert_eq!(said(events), "zroot was imported read/write already");
    let events = machine.apply(import("zroot", Access::ReadOnly, strict, false));
    assert_eq!(said(events), "zroot imported read-only");
    assert!(!pool(&machine, "zroot").writable);

    let events = machine.apply(import("tank", Access::ReadWrite, strict, true));
    assert_eq!(said(events), "tank rewound to its checkpoint");
    assert!(pool(&machine, "tank").writable);
    assert_eq!(pool(&machine, "tank").checkpoint, Some(false));
    let events = machine.apply(import("tank", Access::ReadWrite, strict, true));
    assert_eq!(said(events), "tank has no checkpoint");

    let hostid = zbm_types::ImportPolicy::Hostid;
    let events = machine.apply(import("backup", Access::ReadOnly, strict, false));
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { .. }))
    ));
    machine.apply(import("backup", Access::ReadOnly, hostid, false));
    assert!(pool(&machine, "backup").imported);
}

#[test]
fn an_export_leaves_the_pool_to_import_and_its_environments_go() {
    let mut machine = FakeMachine::new(&DemoOptions::default());
    let zroot: zbm_types::PoolName = "zroot".parse().unwrap();

    let events = machine.apply(Action::Export(zroot.clone()));

    let done = Some(Event::Finished(Outcome::Done(
        "zroot is exported".to_owned(),
    )));
    assert_eq!(events.last(), done.as_ref());
    let snapshot = machine.snapshot();
    let pool = snapshot.pools.iter().find(|p| p.name == zroot).unwrap();
    assert!(!pool.imported && !pool.writable);
    assert!(snapshot.environments.iter().all(|e| e.name.pool() != zroot));
    // The default was on it: there is none now.
    assert_eq!(snapshot.bootfs, None);
    let again = machine.apply(Action::Export(zroot));
    assert!(matches!(
        again.last(),
        Some(Event::Finished(Outcome::Warning(_)))
    ));
}

#[test]
fn the_lost_disk_has_kernel_messages_and_the_others_none() {
    let mut machine = FakeMachine::new(&DemoOptions::default());
    let log = |machine: &mut FakeMachine, path: &str| {
        let device = Untrusted::new(path.to_owned());
        match machine.apply(Action::LoadDeviceLog(device)).as_slice() {
            [Event::DeviceLogLoaded(log)] => log.clone(),
            other => panic!("{other:?}"),
        }
    };

    let lost = log(&mut machine, "/dev/sdb2");
    assert_eq!(lost.names.len(), 2);
    assert!(lost.lines.iter().any(|line| line.get().contains('\u{1b}')));
    let quiet = log(&mut machine, "/dev/sda2");
    assert!(quiet.lines.is_empty() && quiet.names.len() == 1);
    assert!(log(&mut machine, "/tmp/file").names.is_empty());
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

#[test]
fn the_workspace_s_tabs_open_with_digits_and_go_round_with_tab() {
    use crate::menu::caps::Capabilities;
    use crate::menu::keymap::{Map, Tab};
    use crate::menu::model::Screen;
    use crate::menu::theme::Theme;
    use zbm_display::keys::Key;

    let options = DemoOptions {
        workspace: true,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    assert_eq!(session.model().map, Map::Workspace);
    let at = |session: &Session| (session.model().screen, session.model().tab());

    // Down to the second environment, then its snapshots.
    session.key(Key::Char('j'));
    let second = session
        .model()
        .selected_environment()
        .map(|e| e.name.clone());
    session.key(Key::Char('2'));
    assert_eq!(at(&session), (Screen::Snapshots, Tab::Snapshots));
    assert_eq!(session.model().focus, second);
    session.key(Key::Tab);
    assert_eq!(at(&session), (Screen::Pools, Tab::Pools));
    session.key(Key::Tab);
    assert_eq!(at(&session), (Screen::Log, Tab::Log));
    session.key(Key::Tab);
    assert_eq!(at(&session), (Screen::Main, Tab::Boot));
    session.key(Key::BackTab);
    assert_eq!(at(&session), (Screen::Log, Tab::Log));
    session.key(Key::Char('1'));
    // Into the kernels column and out of it.
    session.key(Key::Char('l'));
    assert_eq!(at(&session), (Screen::Kernels, Tab::Boot));
    session.key(Key::Char('2'));
    assert_eq!(session.model().focus, second);
    assert_eq!(at(&session), (Screen::Snapshots, Tab::Snapshots));
    session.key(Key::Char('1'));
    session.key(Key::Char('l'));
    session.key(Key::Char('h'));
    assert_eq!(at(&session), (Screen::Main, Tab::Boot));
}

#[test]
fn the_whole_machine_crosses_to_a_remote_menu_as_it_is() {
    use zbm_app::remote::ToMenu;
    let options = DemoOptions {
        locked: true,
        foreign: true,
        failing: true,
        next_boot: true,
        tpm_changed: true,
        ..DemoOptions::default()
    };
    for options in [DemoOptions::default(), options] {
        let mut machine = FakeMachine::new(&options);
        for _ in 0..50 {
            machine.tick();
        }
        let message = ToMenu::Snapshot(Some(Box::new(machine.snapshot().clone())));
        let packet = message.encode().unwrap();
        assert_eq!(ToMenu::decode(&packet).unwrap(), message);
        let message = ToMenu::Snapshot(Some(Box::new(data::machine(&options))));
        let packet = message.encode().unwrap();
        assert_eq!(ToMenu::decode(&packet).unwrap(), message);
    }
}
