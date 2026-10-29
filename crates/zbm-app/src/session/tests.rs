#![allow(
    clippy::disallowed_methods,
    reason = "the socket is a real one, in a directory of the test's own, served on its own thread"
)]

use std::path::PathBuf;
use std::time::Instant;

use zbm_platform::event::spawn_scoped_traced;

use super::*;

/// A path for the socket, in a new directory of its own.
fn socket_path(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("zbm-app-session-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    directory.join("session.sock")
}

/// Runs `client` while `socket` serves; what it returns.
fn served<T>(socket: &SessionSocket, client: impl FnOnce() -> T) -> T {
    let token = CancelToken::new().unwrap();
    std::thread::scope(|scope| {
        let server = spawn_scoped_traced(scope, || socket.serve(&token));
        let result = client();
        token.cancel();
        server.join().unwrap();
        result
    })
}

#[test]
fn messages_carry_the_version_and_come_back_as_they_were_sent() {
    assert_eq!(
        Request::Takeover.encode(),
        br#"{"request":"takeover","version":1}"#
    );
    assert_eq!(
        Request::decode(&Request::Takeover.encode()),
        Ok(Request::Takeover)
    );
    for answer in [
        Answer::Yielded,
        Answer::Refused(Refusal::NotRoot),
        Answer::Refused(Refusal::Version),
        Answer::Refused(Refusal::Malformed),
        Answer::Refused(Refusal::Busy),
    ] {
        assert_eq!(Answer::decode(&answer.encode()), Some((VERSION, answer)));
    }
}

#[test]
fn a_request_of_another_version_or_none_of_the_protocol_is_refused() {
    let refused = |bytes: &[u8]| Request::decode(bytes).unwrap_err();
    assert_eq!(
        refused(br#"{"version":2,"request":"takeover"}"#),
        Refusal::Version
    );
    assert_eq!(
        refused(br#"{"version":1,"request":"reboot"}"#),
        Refusal::Malformed
    );
    assert_eq!(refused(br#"{"request":"takeover"}"#), Refusal::Malformed);
    assert_eq!(refused(br#"{"version":"1"}"#), Refusal::Malformed);
    assert_eq!(refused(b"[1]"), Refusal::Malformed);
    assert_eq!(refused(b"\xff"), Refusal::Malformed);
    assert_eq!(refused(b""), Refusal::Malformed);

    assert_eq!(Answer::decode(br#"{"version":1,"reply":"done"}"#), None);
    assert_eq!(
        Answer::decode(br#"{"version":1,"reply":"refused","reason":"tired"}"#),
        None
    );
    assert_eq!(Answer::decode(br#"{"version":1,"reply":"refused"}"#), None);
    assert_eq!(Answer::decode(br#"{"version":1}"#), None);
}

#[test]
fn a_session_without_a_menu_has_yielded_at_once() {
    let handover = Handover::default();
    assert!(!handover.is_asked());
    assert!(handover.has_yielded(Duration::from_secs(60)));
    // Nothing runs, so nothing was asked to end.
    handover.enter();
    assert!(!handover.is_asked());
}

#[test]
fn a_menu_asked_to_end_is_waited_for_until_it_does_or_the_wait_is_over() {
    let handover = Handover::default();
    handover.enter();
    assert!(!handover.has_yielded(Duration::from_millis(10)));
    assert!(handover.is_asked());
    // The next menu was not asked.
    handover.leave();
    handover.enter();
    assert!(!handover.is_asked());

    let yielded = std::thread::scope(|scope| {
        spawn_scoped_traced(scope, || {
            while !handover.is_asked() {
                std::thread::sleep(Duration::from_millis(1));
            }
            handover.leave();
        });
        handover.has_yielded(Duration::from_secs(60))
    });
    assert!(yielded);
    assert!(!handover.is_asked());
}

#[test]
fn the_session_ends_its_menu_and_then_says_it_yielded() {
    let path = socket_path("yield");
    let socket = SessionSocket::bind(&path).unwrap();
    let handover = socket.handover();
    handover.enter();

    let answer = served(&socket, || {
        std::thread::scope(|scope| {
            // The menu ends once it is asked to.
            let menu = spawn_scoped_traced(scope, || {
                while !handover.is_asked() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                handover.leave();
            });
            let answer = ask_takeover(&path);
            menu.join().unwrap();
            answer
        })
    });

    assert_eq!(answer, Some(Answer::Yielded));
    // With no menu, it yields at once, and again.
    assert_eq!(
        served(&socket, || [ask_takeover(&path), ask_takeover(&path)]),
        [Some(Answer::Yielded), Some(Answer::Yielded)]
    );
    drop(socket);
    assert!(!path.exists());
}

#[test]
fn a_menu_that_does_not_end_in_time_is_busy() {
    let path = socket_path("busy");
    let socket = SessionSocket::bind(&path)
        .unwrap()
        .with_yield_wait(Duration::from_millis(20));
    socket.handover().enter();

    let started = Instant::now();
    let answer = served(&socket, || ask_takeover(&path));

    assert_eq!(answer, Some(Answer::Refused(Refusal::Busy)));
    assert!(started.elapsed() >= socket.yield_wait);
    assert_eq!(Refusal::Busy.to_string(), "busy");
    assert!(socket.handover().is_asked());
}

#[test]
fn peers_of_another_user_and_requests_of_no_protocol_are_refused() {
    let path = socket_path("refused");
    let mut socket = SessionSocket::bind(&path).unwrap();

    let raw = |bytes: &[u8]| {
        let session = Connection::connect(&path).unwrap();
        session.send(bytes).unwrap();
        let answer = session.receive(Some(ANSWER_WAIT)).unwrap().unwrap();
        Answer::decode(&answer).unwrap().1
    };
    let answers = served(&socket, || {
        [
            raw(b"takeover"),
            raw(br#"{"version":7,"request":"takeover"}"#),
        ]
    });
    assert_eq!(
        answers,
        [
            Answer::Refused(Refusal::Malformed),
            Answer::Refused(Refusal::Version)
        ]
    );
    // Root's session refuses every other user.
    socket.user = socket.user.wrapping_add(1);
    assert_eq!(
        served(&socket, || ask_takeover(&path)),
        Some(Answer::Refused(Refusal::NotRoot))
    );
}

#[test]
fn a_peer_that_goes_before_the_answer_leaves_the_session_serving() {
    let path = socket_path("gone");
    let socket = SessionSocket::bind(&path)
        .unwrap()
        .with_yield_wait(Duration::from_millis(20));
    socket.handover().enter();

    let answer = served(&socket, || {
        let session = Connection::connect(&path).unwrap();
        session.send(&Request::Takeover.encode()).unwrap();
        // Gone while the session waits for its menu.
        drop(session);
        ask_takeover(&path)
    });

    assert_eq!(answer, Some(Answer::Refused(Refusal::Busy)));
}

#[test]
fn a_peer_that_sends_nothing_is_refused_after_a_moment() {
    let path = socket_path("silent");
    let socket = SessionSocket::bind(&path).unwrap();

    let answer = served(&socket, || {
        let session = Connection::connect(&path).unwrap();
        let answer = session.receive(Some(ANSWER_WAIT)).unwrap().unwrap();
        Answer::decode(&answer).map(|(_, answer)| answer)
    });

    assert_eq!(answer, Some(Answer::Refused(Refusal::Malformed)));
}

#[test]
fn without_a_session_nobody_answers() {
    let path = socket_path("none");
    assert_eq!(ask_takeover(&path), None);

    // A session that is not one: it answers nothing of the protocol.
    let listener = Listener::bind(&path).unwrap();
    let answer = std::thread::scope(|scope| {
        spawn_scoped_traced(scope, || {
            let peer = listener.accept(None).unwrap().unwrap();
            let _ = peer.receive(None);
            peer.send(b"hello").unwrap();
        });
        ask_takeover(&path)
    });
    assert_eq!(answer, None);
}
