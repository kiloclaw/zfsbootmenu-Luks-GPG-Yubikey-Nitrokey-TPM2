use std::os::fd::AsFd as _;

use pretty_assertions::assert_eq;

use super::*;
use crate::model::{Method, Stage, StageState};

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
fn a_secret_of_another_length_is_refused() {
    let (menu, session) = Connection::pair().unwrap();
    menu.send(b".{\"version\":1,\"message\":{\"send\":{\"action\":\"Shell\"}},\"secret\":4}")
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
        refused(b".{\"version\":2,\"message\":\"read\"}"),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        refused(b"?{\"version\":1,\"message\":\"read\"}"),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        refused(b".{\"version\":1,\"message\":\"dance\"}"),
        io::ErrorKind::InvalidData
    );
    let bad_name = b".{\"version\":1,\"message\":{\"send\":{\"action\":\
                     {\"SetDefaultEnvironment\":\"zroot//x\"}}}}";
    assert_eq!(refused(bad_name), io::ErrorKind::InvalidData);

    // The same, decoded alone.
    assert!(ToSession::decode(b".{\"version\":1,\"message\":\"cancel\"}").is_ok());
    assert!(ToSession::decode(b"+{\"version\":1,\"message\":\"cancel\"}").is_err());
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
    menu.send(b"+{\"version\":1,").unwrap();
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
