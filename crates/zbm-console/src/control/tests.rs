#![allow(
    clippy::disallowed_methods,
    reason = "the socket is a real one, in a directory of the test's own, and the loop a thread"
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use pretty_assertions::assert_eq;
use zbm_app::console::{Unanswered, ask, hold_secret};
use zbm_platform::event::spawn_scoped_traced;
use zbm_testkit::TempDir;

use super::*;

/// The font the stand-in loop draws with at first, and once larger.
const FONT: &str = "ter-v16b.psf 8x16, 80x25";
const LARGER: &str = "ter-v32b.psf 16x32, 40x12";

/// Waits a moment, as the stand-in loop does between looks.
fn moment() {
    std::thread::sleep(Duration::from_millis(1));
}

/// What a console's loop does with what it is asked, until `stop`: the
/// image has the layouts `us` and `de`, and one larger font.
fn stand_in(control: &Control, stop: &AtomicBool) {
    control.shown("us", FONT);
    while !stop.load(Ordering::SeqCst) {
        if let Some(name) = control.take_layout() {
            let layout = if name == "de" { "de" } else { "us" };
            control.layout_answered(&name, layout);
        }
        if control.take_font().is_some() {
            control.font_shown(LARGER);
        }
        if control.is_release_asked() {
            control.released();
        }
        if control.is_reattach_asked() {
            control.shown("de", LARGER);
        }
        moment();
    }
}

/// Runs `client` while `control` serves the socket at `socket`, and, when
/// `looping`, a stand-in loop does what it asks; what `client` returns.
fn served<T>(
    control: &Arc<Control>,
    socket: &Path,
    looping: bool,
    client: impl FnOnce() -> T,
) -> T {
    let listener = Listener::bind(socket).unwrap();
    let until = CancelToken::new().unwrap();
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        spawn_scoped_traced(scope, || serve(control, &listener, &until));
        if looping {
            spawn_scoped_traced(scope, || stand_in(control, &stop));
        }
        let result = client();
        stop.store(true, Ordering::SeqCst);
        until.cancel();
        result
    })
}

/// A path for the socket, in a new directory of its own.
fn socket(dir: &TempDir) -> PathBuf {
    dir.join("run/console.sock")
}

/// The console's reply to `request`, or why there was none.
fn asked(socket: &Path, request: &Request) -> Result<Reply, Unanswered> {
    ask(socket, request, Duration::from_secs(10))
}

#[test]
fn a_console_answers_once_its_loop_did_what_was_asked() {
    let dir = TempDir::new("zbm-console-control");
    let socket = socket(&dir);
    let (control, _wake) = Control::new().unwrap();

    let replies = served(&control, &socket, true, || {
        while !matches!(asked(&socket, &Request::Status), Ok(Reply::Status(status)) if status.shown)
        {
            moment();
        }
        [
            asked(&socket, &Request::Layout("de".to_owned())),
            asked(&socket, &Request::Layout("zz".to_owned())),
            asked(&socket, &Request::Font("+".to_owned())),
            asked(&socket, &Request::Font("huge".to_owned())),
            asked(&socket, &Request::Status),
            asked(&socket, &Request::Reattach),
            asked(&socket, &Request::Release),
            asked(&socket, &Request::Release),
            asked(&socket, &Request::Layout("de".to_owned())),
            asked(&socket, &Request::Font("-".to_owned())),
            asked(&socket, &Request::Status),
            asked(&socket, &Request::Reattach),
            asked(&socket, &Request::Status),
        ]
    });

    let status = |shown, layout: &str, font: &str| {
        Ok(Reply::Status(Status {
            shown,
            layout: layout.to_owned(),
            font: font.to_owned(),
        }))
    };
    assert_eq!(
        replies,
        [
            Ok(Reply::LayoutSet("de".to_owned())),
            Ok(Reply::NoSuchLayout("zz".to_owned())),
            Ok(Reply::FontSet(LARGER.to_owned())),
            Ok(Reply::Refused(Refusal::Malformed)),
            status(true, "us", LARGER),
            // Shown, so nothing to take again.
            Ok(Reply::Reattached),
            Ok(Reply::Released),
            // Handed back already.
            Ok(Reply::Released),
            Ok(Reply::Refused(Refusal::NotShown)),
            Ok(Reply::Refused(Refusal::NotShown)),
            status(false, "us", LARGER),
            Ok(Reply::Reattached),
            status(true, "de", LARGER),
        ]
    );
    // The socket went with its listener.
    assert_eq!(asked(&socket, &Request::Status), Err(Unanswered::NoConsole));
}

#[test]
fn a_secret_prompt_is_held_until_its_connection_closes() {
    let dir = TempDir::new("zbm-console-secret");
    let socket = socket(&dir);
    let (control, mut wake) = Control::new().unwrap();

    served(&control, &socket, false, || {
        let held = hold_secret(&socket).unwrap();
        assert!(control.is_secret());
        let other = hold_secret(&socket).unwrap();
        drop(held);
        // The other still holds it.
        std::thread::sleep(Duration::from_millis(20));
        assert!(control.is_secret());
        drop(other);
        while control.is_secret() {
            moment();
        }
        // Each change woke the loop.
        let readable = zbm_sys::console::first_readable(&[wake.fd()], Some(Duration::ZERO));
        assert_eq!(readable.unwrap(), Some(0));
        wake.take();
        let readable = zbm_sys::console::first_readable(&[wake.fd()], Some(Duration::ZERO));
        assert_eq!(readable.unwrap(), None);
    });
    assert!(format!("{control:?} {wake:?}").starts_with("Control"));
}

#[test]
fn what_the_loop_does_not_do_in_time_is_busy() {
    let dir = TempDir::new("zbm-console-busy");
    let socket = socket(&dir);
    let (control, _wake) = Control::waiting(Duration::from_millis(20)).unwrap();
    control.shown("us", FONT);

    let replies = served(&control, &socket, false, || {
        [
            asked(&socket, &Request::Layout("de".to_owned())),
            asked(&socket, &Request::Font("+".to_owned())),
            asked(&socket, &Request::Release),
        ]
    });

    assert_eq!(replies, [const { Ok(Reply::Refused(Refusal::Busy)) }; 3]);
    // Asked from the loop's side, the secret is no request to answer.
    assert_eq!(
        control.answer(&Request::Secret),
        Reply::Refused(Refusal::Malformed)
    );
}

#[test]
fn a_console_that_relays_for_good_has_nothing_to_hand_back_or_take_again() {
    let (control, _wake) = Control::waiting(Duration::from_secs(10)).unwrap();
    control.relaying();
    assert_eq!(control.answer(&Request::Release), Reply::Released);
    assert_eq!(control.answer(&Request::Reattach), Reply::Reattached);
    assert_eq!(
        control.answer(&Request::Layout("de".to_owned())),
        Reply::Refused(Refusal::NotShown)
    );

    // Handed back, then the display could not be had again.
    control.released();
    let reattached = std::thread::scope(|scope| {
        let asking = spawn_scoped_traced(scope, || control.answer(&Request::Reattach));
        while !control.is_reattach_asked() {
            moment();
        }
        control.relaying();
        asking.join().unwrap()
    });
    assert_eq!(reattached, Reply::Refused(Refusal::NotShown));

    // A console that ends answers what waits.
    control.released();
    let ended = std::thread::scope(|scope| {
        let asking = spawn_scoped_traced(scope, || control.answer(&Request::Reattach));
        while !control.is_reattach_asked() {
            moment();
        }
        control.end();
        asking.join().unwrap()
    });
    assert_eq!(ended, Reply::Refused(Refusal::Busy));
}

#[test]
fn peers_of_another_user_and_requests_of_no_protocol_are_refused() {
    let dir = TempDir::new("zbm-console-refused");
    let socket = socket(&dir);
    let (control, _wake) = Control::new().unwrap();

    let raw = |bytes: &[u8]| {
        let console = Connection::connect(&socket).unwrap();
        console.send(bytes).unwrap();
        let reply = console
            .receive(Some(Duration::from_secs(10)))
            .unwrap()
            .unwrap();
        Reply::decode(&reply).unwrap().1
    };
    let replies = served(&control, &socket, false, || {
        let silent = Connection::connect(&socket).unwrap();
        let unanswered = silent
            .receive(Some(Duration::from_secs(10)))
            .unwrap()
            .unwrap();
        [
            raw(b"release"),
            raw(br#"{"version":9,"request":"release"}"#),
            Reply::decode(&unanswered).unwrap().1,
        ]
    });

    assert_eq!(
        replies,
        [
            Reply::Refused(Refusal::Malformed),
            Reply::Refused(Refusal::Version),
            Reply::Refused(Refusal::Malformed),
        ]
    );
    let peer = Connection::pair().unwrap();
    std::thread::scope(|scope| {
        spawn_scoped_traced(scope, || control.answer_peer(&peer.1, u32::MAX));
        peer.0.send(&Request::Status.encode()).unwrap();
        let reply = peer
            .0
            .receive(Some(Duration::from_secs(10)))
            .unwrap()
            .unwrap();
        assert_eq!(
            Reply::decode(&reply).unwrap().1,
            Reply::Refused(Refusal::NotRoot)
        );
    });
}
