use std::os::fd::AsFd as _;

use pretty_assertions::assert_eq;
use zbm_types::Untrusted;

use super::*;
use crate::model::{CommandLine, Method, Origin, Stage, StageState};

const WAIT: Option<Duration> = Some(Duration::from_secs(5));

fn dataset(name: &str) -> zbm_types::DatasetName {
    name.parse().unwrap()
}

/// A snapshot several packets long.
fn long_snapshot() -> Snapshot {
    let stage = Stage {
        name: "x".repeat(1000),
        state: StageState::Pending,
    };
    Snapshot {
        startup: vec![stage; 200],
        bootfs: Some(dataset("zroot/ROOT/void")),
        now: 1_700_000_000,
        offers: [crate::model::Offer::Boot, crate::model::Offer::Power]
            .into_iter()
            .collect(),
        ..Snapshot::default()
    }
}

fn unlock(secret: Option<&[u8]>) -> Action {
    Action::Unlock {
        volume: "root".to_owned(),
        method: Method::Passphrase,
        secret: secret.map(|bytes| Arc::new(Secret::from_vec(bytes.to_vec()))),
    }
}

#[test]
fn a_long_snapshot_crosses_in_parts_and_comes_back_whole() {
    let (menu, session) = Connection::pair().unwrap();
    let snapshot = ToMenu::Snapshot(Some(Box::new(long_snapshot())));
    // More than the socket holds: the session waits for the menu to read.
    let sent = snapshot.clone();
    let sender = zbm_platform::event::spawn_traced("sender", move || {
        sent.send(&session).unwrap();
        ToMenu::Done.send(&session).unwrap();
        session
    })
    .unwrap();

    assert_eq!(ToMenu::receive(&menu, WAIT).unwrap(), Some(snapshot));
    assert_eq!(ToMenu::receive(&menu, WAIT).unwrap(), Some(ToMenu::Done));
    let _session = sender.join().unwrap();
    assert_eq!(ToMenu::receive(&menu, Some(Duration::ZERO)).unwrap(), None);
}

#[test]
fn attaching_hands_the_session_the_terminal() {
    let (menu, session) = Connection::pair().unwrap();
    let (input, output) = Connection::pair().unwrap();
    ToSession::Attach
        .send(&menu, &[input.as_fd(), output.as_fd()])
        .unwrap();

    let received = ToSession::receive(&session, WAIT).unwrap().unwrap();
    assert_eq!(received.message, ToSession::Attach);
    assert_eq!(received.fds.len(), 2);
    ToSession::Read.send(&menu, &[]).unwrap();
    let received = ToSession::receive(&session, WAIT).unwrap().unwrap();
    assert_eq!((received.message, received.fds.len()), (ToSession::Read, 0));
}

#[test]
fn a_secret_follows_as_a_packet_of_its_own() {
    let (menu, session) = Connection::pair().unwrap();
    let action = Action::Plan(Box::new(unlock(Some(b"hunter2"))));
    ToSession::Send {
        action: action.clone(),
    }
    .send(&menu, &[])
    .unwrap();

    // On the wire: the message without it, then the secret alone.
    let message = session.receive(WAIT).unwrap().unwrap();
    let text = String::from_utf8(message.clone()).unwrap();
    assert!(!text.contains("hunter2"), "{text}");
    assert!(text.contains("\"secret\":7"), "{text}");
    let secret = session.receive(WAIT).unwrap().unwrap();
    assert_eq!(secret, b"hunter2");

    // Read as one, the secret is back in its action.
    let (menu, session) = Connection::pair().unwrap();
    ToSession::Console {
        action: action.clone(),
    }
    .send(&menu, &[])
    .unwrap();
    let received = ToSession::receive(&session, WAIT).unwrap().unwrap();
    assert_eq!(received.message, ToSession::Console { action });

    // An action without a secret sends none.
    ToSession::Send {
        action: unlock(None),
    }
    .send(&menu, &[])
    .unwrap();
    let received = ToSession::receive(&session, WAIT).unwrap().unwrap();
    assert_eq!(
        received.message,
        ToSession::Send {
            action: unlock(None)
        }
    );
    assert_eq!(session.receive(Some(Duration::ZERO)).unwrap(), None);

    // An empty secret is a secret, sent as no packet.
    ToSession::Send {
        action: unlock(Some(b"")),
    }
    .send(&menu, &[])
    .unwrap();
    ToSession::Cancel.send(&menu, &[]).unwrap();
    let received = ToSession::receive(&session, WAIT).unwrap().unwrap();
    assert_eq!(
        received.message,
        ToSession::Send {
            action: unlock(Some(b""))
        }
    );
    let received = ToSession::receive(&session, WAIT).unwrap().unwrap();
    assert_eq!(received.message, ToSession::Cancel);
}

#[test]
fn an_answer_carries_its_secret_as_a_packet_of_its_own() {
    let (menu, session) = Connection::pair().unwrap();
    let answer = ToSession::Answer {
        question: 1,
        action: unlock(Some(b"hunter2")),
    };
    answer.clone().send(&menu, &[]).unwrap();
    answer.clone().send(&menu, &[]).unwrap();

    let message = session.receive(WAIT).unwrap().unwrap();
    let text = String::from_utf8(message).unwrap();
    assert!(
        text.contains("\"answer\"") && !text.contains("hunter2"),
        "{text}"
    );
    assert_eq!(session.receive(WAIT).unwrap().unwrap(), b"hunter2");
    let received = ToSession::receive(&session, WAIT).unwrap().unwrap();
    assert_eq!(received.message, answer);
}

#[test]
fn a_secret_of_another_length_is_refused() {
    let (menu, session) = Connection::pair().unwrap();
    menu.send(b".{\"version\":2,\"message\":{\"send\":{\"action\":\"Shell\"}},\"secret\":4}")
        .unwrap();
    menu.send(b"12").unwrap();
    let error = ToSession::receive(&session, WAIT).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn what_is_not_of_the_protocol_is_refused() {
    let (menu, session) = Connection::pair().unwrap();
    let refused = |packet: &[u8]| {
        menu.send(packet).unwrap();
        ToSession::receive(&session, WAIT).unwrap_err().kind()
    };
    // Another version, no part, JSON of nothing here, a name its parser
    // refuses.
    assert_eq!(
        refused(b".{\"version\":1,\"message\":\"read\"}"),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        refused(b"?{\"version\":2,\"message\":\"read\"}"),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        refused(b".{\"version\":2,\"message\":\"dance\"}"),
        io::ErrorKind::InvalidData
    );
    let bad_name = b".{\"version\":2,\"message\":{\"send\":{\"action\":\
                     {\"SetDefaultEnvironment\":\"zroot//x\"}}}}";
    assert_eq!(refused(bad_name), io::ErrorKind::InvalidData);

    // The same, decoded alone.
    assert!(ToSession::decode(b".{\"version\":2,\"message\":\"cancel\"}").is_ok());
    assert!(ToSession::decode(b"+{\"version\":2,\"message\":\"cancel\"}").is_err());
    assert!(ToSession::decode(b"").is_err());
    let encoded = ToMenu::Refused("no".to_owned()).encode().unwrap();
    assert_eq!(
        ToMenu::decode(&encoded).unwrap(),
        ToMenu::Refused("no".to_owned())
    );
    assert!(ToMenu::decode(&encoded[1..]).is_err());
}

#[test]
fn a_menu_that_goes_mid_message_is_gone() {
    let (menu, session) = Connection::pair().unwrap();
    menu.send(b"+{\"version\":2,").unwrap();
    drop(menu);
    let error = ToSession::receive(&session, WAIT).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    let (menu, session) = Connection::pair().unwrap();
    drop(session);
    assert!(ToMenu::Done.send(&menu).is_err());
}

#[test]
fn no_message_is_longer_than_the_limit() {
    let (menu, session) = Connection::pair().unwrap();
    let long = vec![b' '; MAX_LENGTH + 1];
    let error = send_parts(&menu, &long, &[]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    // A peer that sends parts without end is refused once they pass it.
    let part = vec![MORE; MAX_MESSAGE];
    let sender = zbm_platform::event::spawn_traced("sender", move || {
        // Until the session stops reading and goes.
        while menu.send(&part).is_ok() {}
    })
    .unwrap();
    let error = ToSession::receive(&session, WAIT).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    drop(session);
    sender.join().unwrap();
}

#[test]
fn an_empty_message_is_one_last_part() {
    let (menu, session) = Connection::pair().unwrap();
    send_parts(&menu, b"", &[]).unwrap();
    assert_eq!(session.receive(WAIT).unwrap().unwrap(), b".");
}

#[test]
fn a_message_encodes_without_its_secret() {
    let message = ToSession::Send {
        action: unlock(Some(b"hunter2")),
    };
    let encoded = message.encode().unwrap();
    assert!(!String::from_utf8_lossy(&encoded).contains("hunter2"));
    assert_eq!(
        ToSession::decode(&encoded).unwrap(),
        ToSession::Send {
            action: unlock(None)
        }
    );
}

#[test]
fn an_export_a_devices_messages_and_snapshots_cross_as_they_are() {
    let device = zbm_types::Untrusted::new("/dev/sda2".to_owned());
    for action in [
        Action::Export("tank".parse().unwrap()),
        Action::LoadDeviceLog(device.clone()),
        Action::LoadSnapshots("tank/ROOT/be".parse().unwrap()),
    ] {
        let message = ToSession::Send { action };
        assert_eq!(
            ToSession::decode(&message.encode().unwrap()).unwrap(),
            message
        );
    }
    let log = crate::model::DeviceLog {
        device,
        names: vec![zbm_types::Untrusted::new("sda2".to_owned())],
        lines: vec![zbm_types::Untrusted::new(
            "[ 1.0] sda2: \u{1b}[2J".to_owned(),
        )],
    };
    let message = ToMenu::Event(Box::new(Event::DeviceLogLoaded(log)));
    assert_eq!(ToMenu::decode(&message.encode().unwrap()).unwrap(), message);
    let list = crate::model::SnapshotList {
        environment: "tank/ROOT/be".parse().unwrap(),
        snapshots: vec![crate::model::SnapshotInfo {
            name: "tank/ROOT/be@new".parse().unwrap(),
            created: 200,
            used: 4096,
        }],
    };
    let message = ToMenu::Event(Box::new(Event::SnapshotsLoaded(list)));
    assert_eq!(ToMenu::decode(&message.encode().unwrap()).unwrap(), message);
    let unread = Event::SnapshotsUnread("tank/ROOT/be".parse().unwrap());
    let message = ToMenu::Event(Box::new(unread));
    assert_eq!(ToMenu::decode(&message.encode().unwrap()).unwrap(), message);
}

#[test]
fn the_boot_history_crosses_as_it_is_read() {
    use crate::model::{History, Recorded, SavedBoot};
    use zbm_types::{BootRecord, Untrusted};

    let message = ToSession::Send {
        action: Action::LoadHistory,
    };
    assert_eq!(
        ToSession::decode(&message.encode().unwrap()).unwrap(),
        message
    );
    let hostile = |text: &str| Untrusted::new(format!("{text}\u{1b}[2J\u{7}"));
    let record = BootRecord {
        time: 1_793_246_955,
        environment: hostile("tank/ROOT/be"),
        kernel: hostile("/boot/vmlinuz"),
        devicetree: Some(hostile("/boot/dtb")),
        ..BootRecord::default()
    };
    let boot = |record| SavedBoot {
        name: hostile("2026-10-23T08-00-00Z"),
        files: vec![(hostile("boot.json"), 300)],
        record,
    };
    for history in [
        History::Off,
        History::Unavailable {
            why: "no EFI system partition found".to_owned(),
            code: 621,
        },
        History::Saved(vec![
            boot(Recorded::Read(record)),
            boot(Recorded::Missing),
            boot(Recorded::Unreadable(
                "the record has schema 2, not 1".to_owned(),
            )),
        ]),
    ] {
        let message = ToMenu::Event(Box::new(Event::HistoryLoaded(history)));
        assert_eq!(ToMenu::decode(&message.encode().unwrap()).unwrap(), message);
    }
}

#[test]
fn the_longest_history_read_fits_in_a_message() {
    use crate::model::{History, Recorded, SavedBoot};
    use zbm_boot::saved_logs::{FILES_READ, READ_BOOTS};
    use zbm_types::{BootRecord, Untrusted};

    // Each record as long as one may be, all of it a character JSON
    // escapes as six; each file named as long as FAT allows, the same way.
    let escaped = |len: usize| Untrusted::new("\u{1}".repeat(len));
    let record = BootRecord {
        command_line: escaped(BootRecord::MAX_LEN),
        ..BootRecord::default()
    };
    let boot = SavedBoot {
        name: escaped(255),
        files: vec![(escaped(255), u64::MAX); FILES_READ],
        record: Recorded::Read(record),
    };
    let history = History::Saved(vec![boot; READ_BOOTS]);

    let message = ToMenu::Event(Box::new(Event::HistoryLoaded(history)));
    assert!(message.encode().unwrap().len() <= MAX_LENGTH);
}

#[test]
fn a_plan_crosses_with_its_review_and_one_without_reads_as_reviewed_empty() {
    use crate::model::{
        Keep, Loss, Outcome, Plan, PoolHost, PoolUse, Review, Safer, Step, StepKind, Stop,
    };
    use zbm_types::Untrusted;

    let snapshot = |name: &str| -> zbm_types::SnapshotName { name.parse().unwrap() };
    let pool: zbm_types::PoolName = "tank".parse().unwrap();
    let hostile = || Untrusted::new("x\u{1b}[2J".to_owned());
    let review = Review {
        loses: vec![
            Loss::Snapshot {
                name: snapshot("tank/ROOT/be@new"),
                used: Some(4096),
            },
            Loss::Bookmark {
                dataset: dataset("tank/ROOT/be"),
                name: "new".to_owned(),
            },
            Loss::Written {
                since: snapshot("tank/ROOT/be@old"),
                bytes: None,
            },
            Loss::Newer(snapshot("tank/ROOT/be@old")),
            Loss::SinceCheckpoint {
                created: Some(hostile()),
                space: Some(1),
                discarding: true,
                datasets: Some(vec![dataset("tank/new")]),
                snapshots: None,
            },
        ],
        freed: Some(4096),
        keeps: vec![Keep::Snapshots(2), Keep::Bookmarks(1)],
        undo: Some(Box::new(Action::Export(pool.clone()))),
        host_id_stays: true,
        safer: Some(Safer::Clone(snapshot("tank/ROOT/be@old"))),
        stops: vec![
            Stop::Clone {
                snapshot: snapshot("tank/ROOT/be@new"),
                clone: hostile(),
            },
            Stop::Hold {
                snapshot: snapshot("tank/ROOT/be@new"),
                tag: None,
            },
            Stop::Active {
                pool,
                host: Some(PoolHost {
                    hostid: None,
                    hostname: Some(hostile()),
                }),
            },
        ],
        pool_use: Some(PoolUse {
            written: Some(1_700_000_000),
            active: true,
            ..PoolUse::default()
        }),
    };
    let rollback = Action::Rollback(snapshot("tank/ROOT/be@old"));
    let plan = Plan {
        steps: vec![Step {
            kind: StepKind::Rollback(snapshot("tank/ROOT/be@old")),
            command: Some("zfs rollback -r tank/ROOT/be@old".to_owned()),
        }],
        outcome: Outcome::Done(String::new()),
        review,
    };
    let planned = Event::Planned {
        action: Box::new(rollback),
        plan: plan.clone(),
    };
    let message = ToMenu::Event(Box::new(planned));
    assert_eq!(ToMenu::decode(&message.encode().unwrap()).unwrap(), message);
    let safer = Safer::Import(Box::new(Action::Reboot));
    let encoded = serde_json::to_string(&safer).unwrap();
    assert_eq!(serde_json::from_str::<Safer>(&encoded).unwrap(), safer);

    // A plan of the same version, 2, without a review, as one sent before
    // reviews came, reads as one that reviews nothing.
    let bare = serde_json::json!({
        "steps": [],
        "outcome": {"Done": "x"},
    });
    let read: Plan = serde_json::from_value(bare).unwrap();
    assert_eq!(read.review, Review::default());
}

#[test]
fn a_snapshot_crosses_with_the_proof_s_time_and_one_without_it_still_reads() {
    let snapshot = Snapshot {
        proof_left: Some(42),
        ..long_snapshot()
    };
    let message = ToMenu::Snapshot(Some(Box::new(snapshot.clone())));
    assert_eq!(ToMenu::decode(&message.encode().unwrap()).unwrap(), message);

    // A snapshot of the same version without the field, as an older peer
    // sends it, reads as one with no proof.
    let mut older = serde_json::to_value(&snapshot).unwrap();
    let removed = older.as_object_mut().unwrap().remove("proof_left");
    assert_eq!(removed, Some(serde_json::json!(42)));
    let read: Snapshot = serde_json::from_value(older).unwrap();
    assert_eq!(read.proof_left, None);
    assert_eq!(read.now, snapshot.now);
}

#[test]
fn a_command_line_crosses_with_its_expansion_and_one_without_it_reads_as_its_value() {
    let line = |value: &str, expanded: &str| CommandLine {
        value: Untrusted::new(value.to_owned()),
        origin: Origin::Inherited(dataset("zroot/ROOT")),
        expanded: Untrusted::new(expanded.to_owned()),
    };
    let sent = line("%{parent} loglevel=4", "spl_hostid=1 loglevel=4");
    let encoded = serde_json::to_value(&sent).unwrap();
    let crossed: CommandLine = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(crossed, sent);

    // A line without the field, as an older peer sends it, reads as its
    // value, which that peer's menu showed.
    let mut older = encoded;
    let removed = older.as_object_mut().unwrap().remove("expanded");
    assert_eq!(removed, Some(serde_json::json!("spl_hostid=1 loglevel=4")));
    let read: CommandLine = serde_json::from_value(older).unwrap();
    assert_eq!(read, line("%{parent} loglevel=4", "%{parent} loglevel=4"));
}
