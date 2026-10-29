#![allow(
    clippy::disallowed_methods,
    reason = "the consoles are real sockets, in a directory of the test's own, answered by a thread"
)]

use std::path::PathBuf;

use pretty_assertions::assert_eq;
use zbm_platform::event::spawn_scoped_traced;
use zbm_sys::socket::Listener;

use super::*;

/// A path for a console's socket, in a new directory of its own.
fn socket_path(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("zbm-app-console-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    directory.join("console.sock")
}

/// What a console at `path` that answers `answer` to the first request
/// gives the client `client`.
fn answered_with<T>(path: &Path, answer: &[u8], client: impl FnOnce() -> T) -> T {
    let listener = Listener::bind(path).unwrap();
    std::thread::scope(|scope| {
        spawn_scoped_traced(scope, || {
            let peer = listener.accept(None).unwrap().unwrap();
            let _ = peer.receive(None);
            peer.send(answer).unwrap();
            // Kept open until the client is done with it.
            let _ = peer.receive(None);
        });
        client()
    })
}

#[test]
fn every_message_comes_back_as_it_was_sent() {
    let requests = [
        Request::Release,
        Request::Reattach,
        Request::Layout("de".to_owned()),
        Request::Font("+".to_owned()),
        Request::Status,
        Request::Secret,
    ];
    for request in requests {
        assert_eq!(Request::decode(&request.encode()), Ok(request));
    }
    let status = Status {
        shown: true,
        layout: "us".to_owned(),
        font: "ter-v16b.psf 8x16, 80x25".to_owned(),
    };
    let mut replies = vec![
        Reply::Released,
        Reply::Reattached,
        Reply::LayoutSet("de".to_owned()),
        Reply::NoSuchLayout("zz".to_owned()),
        Reply::FontSet("ter-v32b.psf 16x32, 40x12".to_owned()),
        Reply::Status(status),
        Reply::SecretHeld,
    ];
    replies.extend(REFUSALS.map(Reply::Refused));
    for reply in replies {
        assert_eq!(Reply::decode(&reply.encode()), Some((VERSION, reply)));
    }
    assert_eq!(
        Request::Layout("de".to_owned()).encode(),
        br#"{"name":"de","request":"layout","version":1}"#
    );
    assert_eq!(Refusal::NotShown.to_string(), "not shown");
}

#[test]
fn a_request_of_another_version_or_none_of_the_protocol_is_refused() {
    let refused = |bytes: &[u8]| Request::decode(bytes).unwrap_err();
    assert_eq!(
        refused(br#"{"version":2,"request":"release"}"#),
        Refusal::Version
    );
    for malformed in [
        &br#"{"version":1,"request":"reboot"}"#[..],
        br#"{"version":1}"#,
        br#"{"version":1,"request":"layout"}"#,
        br#"{"version":1,"request":"layout","name":""}"#,
        br#"{"version":1,"request":"layout","name":"../../etc/shadow"}"#,
        br#"{"version":1,"request":"font"}"#,
        br#"{"request":"release"}"#,
        b"release",
    ] {
        assert_eq!(refused(malformed), Refusal::Malformed, "{malformed:?}");
    }
    let long = format!(
        r#"{{"version":1,"request":"layout","name":"{}"}}"#,
        "a".repeat(65)
    );
    assert_eq!(refused(long.as_bytes()), Refusal::Malformed);

    for garbled in [
        &br#"{"version":1,"reply":"done"}"#[..],
        br#"{"version":1,"reply":"layout set"}"#,
        br#"{"version":1,"reply":"status","shown":"yes","layout":"us","font":"f"}"#,
        br#"{"version":1,"reply":"status","shown":true,"font":"f"}"#,
        br#"{"version":1,"reply":"refused","reason":"tired"}"#,
        br#"{"version":1}"#,
    ] {
        assert_eq!(Reply::decode(garbled), None, "{garbled:?}");
    }
}

#[test]
fn a_request_without_a_console_or_a_reply_says_why() {
    let path = socket_path("none");
    let wait = Duration::from_secs(5);
    assert_eq!(
        ask(&path, &Request::Status, wait),
        Err(Unanswered::NoConsole)
    );
    assert!(hold_secret(&path).is_none());

    // A console that never answers.
    let silent = Listener::bind(&path).unwrap();
    let short = Duration::from_millis(20);
    assert_eq!(ask(&path, &Request::Release, short), Err(Unanswered::Late));
    drop(silent);

    let garbled = answered_with(&path, b"hello", || ask(&path, &Request::Status, wait));
    assert_eq!(garbled, Err(Unanswered::Garbled));
    let newer = br#"{"version":2,"reply":"released"}"#;
    let later = answered_with(&path, newer, || ask(&path, &Request::Release, wait));
    assert_eq!(later, Err(Unanswered::Garbled));
    // A refusal of the version is understood in any version.
    let refusal = br#"{"version":2,"reply":"refused","reason":"version"}"#;
    let refused = answered_with(&path, refusal, || ask(&path, &Request::Release, wait));
    assert_eq!(refused, Ok(Reply::Refused(Refusal::Version)));
    let gone = answered_with(&path, b"", || ask(&path, &Request::Release, wait));
    assert_eq!(gone, Err(Unanswered::NoConsole));
    // A secret not noted is not held.
    let busy = Reply::Refused(Refusal::Busy).encode();
    assert!(answered_with(&path, &busy, || hold_secret(&path)).is_none());
    let held = Reply::SecretHeld.encode();
    assert!(answered_with(&path, &held, || hold_secret(&path).map(drop)).is_some());

    assert_eq!(
        [
            Unanswered::NoConsole.to_string(),
            Unanswered::Late.to_string(),
            Unanswered::Garbled.to_string(),
        ],
        [
            "no console on the display",
            "the console did not answer",
            "the console's answer was not understood",
        ]
    );
}
