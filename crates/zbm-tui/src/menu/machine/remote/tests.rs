#![allow(
    clippy::disallowed_methods,
    reason = "the session is played by a thread of the test's own, on a real socket pair"
)]

use std::sync::atomic::{AtomicBool, Ordering};

use pretty_assertions::assert_eq;
use zbm_app::remote::TAKEN_OVER;
use zbm_platform::event::spawn_scoped_traced;
use zbm_sys::socket::Connection;

use super::*;
use crate::menu::world::Outcome;

const WAIT: Option<Duration> = Some(Duration::from_secs(5));

/// What the menu sends next, to the session.
fn next(session: &Connection) -> ToSession {
    ToSession::receive(session, WAIT).unwrap().unwrap().message
}

/// A machine the session would send: it counts down.
fn counting() -> Snapshot {
    Snapshot {
        timeout: Timeout::Seconds(10),
        ..Snapshot::default()
    }
}

/// The session's side of an attach: the terminal comes, then a read,
/// answered after `before`.
fn attached(session: &Connection, before: &[ToMenu]) {
    let received = ToSession::receive(session, WAIT).unwrap().unwrap();
    assert_eq!(
        (received.message, received.fds.len()),
        (ToSession::Attach, 2)
    );
    ToMenu::Attached.send(session).unwrap();
    assert_eq!(next(session), ToSession::Read);
    for message in before {
        message.send(session).unwrap();
    }
}

fn never() -> bool {
    false
}

/// A machine attached to a session that `play` plays, until `menu`
/// returns.
fn with_session<T>(
    play: impl FnOnce(&Connection) + Send,
    menu: impl FnOnce(io::Result<RemoteMachine<'_>>) -> T,
) -> T {
    let (ours, session) = Connection::pair().unwrap();
    let (input, output) = Connection::pair().unwrap();
    std::thread::scope(|scope| {
        let player = spawn_scoped_traced(scope, move || play(&session));
        let machine = RemoteMachine::attach(ours, (input.as_fd(), output.as_fd()), &never);
        let result = menu(machine);
        player.join().unwrap();
        result
    })
}

/// The events of `machine` until it is no longer busy.
fn until_idle(machine: &mut RemoteMachine<'_>) -> Vec<Event> {
    let mut events = Vec::new();
    while machine.is_busy() {
        std::thread::sleep(Duration::from_millis(1));
        events.extend(machine.poll());
    }
    events
}

#[test]
fn the_menu_reads_the_machine_and_its_actions_are_performed_meanwhile() {
    let take = Action::CreateSnapshot("tank/ROOT/be@new".parse().unwrap());
    let sent = take.clone();
    let events = with_session(
        move |session| {
            attached(session, &[ToMenu::Snapshot(Some(Box::new(counting())))]);
            assert_eq!(next(session), ToSession::Send { action: sent });
            ToMenu::Event(Box::new(Event::Changed(counting())))
                .send(session)
                .unwrap();
            ToMenu::Event(Box::new(Event::Finished(Outcome::Cancelled)))
                .send(session)
                .unwrap();
            ToMenu::Done.send(session).unwrap();
            assert_eq!(next(session), ToSession::Cancel);
            assert_eq!(next(session), ToSession::Detach);
        },
        |machine| {
            let mut machine = machine.unwrap();
            // The countdown does not run again.
            assert_eq!(machine.snapshot().timeout, Timeout::Wait);
            assert!(machine.waker().is_some());
            let first = machine.tick();
            assert!(matches!(first.as_slice(), [Event::Changed(_)]), "{first:?}");
            assert!(machine.apply(take).is_empty());
            assert!(machine.is_busy());
            let events = until_idle(&mut machine);
            machine.cancel();
            assert_eq!(machine.snapshot().timeout, Timeout::Wait);
            events
        },
    );
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(matches!(&events[0], Event::Changed(snapshot) if snapshot.timeout == Timeout::Wait));
    assert_eq!(events[1], Event::Finished(Outcome::Cancelled));
}

#[test]
fn an_action_that_takes_the_console_waits_until_the_session_performed_it() {
    let reboot = Action::Reboot;
    let events = with_session(
        |session| {
            attached(session, &[ToMenu::Snapshot(Some(Box::default()))]);
            assert_eq!(
                next(session),
                ToSession::Console {
                    action: Action::Reboot
                }
            );
            ToMenu::Event(Box::new(Event::Finished(Outcome::Cancelled)))
                .send(session)
                .unwrap();
            ToMenu::Done.send(session).unwrap();
            // Failed, as when the service ended.
            assert!(matches!(next(session), ToSession::Send { .. }));
            let failed = Event::Finished(Outcome::Failed {
                what: "unable to perform the action".to_owned(),
                why: vec!["the machine's service ended".to_owned()],
                next: Vec::new(),
            });
            ToMenu::Event(Box::new(failed)).send(session).unwrap();
            ToMenu::Done.send(session).unwrap();
            assert_eq!(next(session), ToSession::Detach);
        },
        |machine| {
            let mut machine = machine.unwrap();
            assert!(machine.needs_console(&reboot));
            assert!(!machine.needs_console(&Action::Shell));
            let mut events = machine.apply(reboot);
            assert!(!machine.is_busy());
            // The recovery shell ends the menu.
            assert_eq!(
                machine.apply(Action::Shell),
                [Event::Leaving(String::new())]
            );
            machine.apply(Action::ResetBootTries("tank/ROOT/be".parse().unwrap()));
            events.extend(until_idle(&mut machine));
            events
        },
    );
    assert_eq!(events[0], Event::Finished(Outcome::Cancelled));
    let Event::Finished(Outcome::Failed { why, .. }) = &events[1] else {
        panic!("{events:?}");
    };
    assert!(why[0].contains("service"), "{why:?}");
}

#[test]
fn a_menu_taken_over_or_whose_session_went_leaves() {
    let leaving = vec![Event::Leaving(String::new())];
    let events = with_session(
        |session| {
            attached(session, &[ToMenu::Snapshot(Some(Box::default()))]);
            ToMenu::Refused(TAKEN_OVER.to_owned())
                .send(session)
                .unwrap();
        },
        |machine| {
            let mut machine = machine.unwrap();
            machine.tick();
            let mut events = Vec::new();
            while events.is_empty() {
                events = machine.tick();
            }
            events
        },
    );
    assert_eq!(events, leaving);

    // The session went: the menu leaves, whatever it does.
    let events = with_session(
        |session| attached(session, &[ToMenu::Snapshot(Some(Box::default()))]),
        |machine| {
            let mut machine = machine.unwrap();
            machine.tick();
            let mut events = Vec::new();
            while events.is_empty() {
                events = machine.poll();
            }
            (events, machine.apply(Action::Reboot))
        },
    );
    assert_eq!(events, (leaving.clone(), leaving));
}

#[test]
fn a_session_that_does_not_serve_the_menu_says_why() {
    let error = with_session(
        |session| attached(session, &[ToMenu::Refused("not served".to_owned())]),
        |machine| machine.err().unwrap(),
    );
    assert_eq!(error.to_string(), "not served");
    let error = with_session(
        |session| attached(session, &[ToMenu::Snapshot(None)]),
        |machine| machine.err().unwrap(),
    );
    assert_eq!(error.to_string(), "the machine could not be read");
    let error = with_session(
        |session| attached(session, &[ToMenu::Done]),
        |machine| machine.err().unwrap(),
    );
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    // A menu that is to end while the session runs the menu program.
    let (ours, session) = Connection::pair().unwrap();
    let ending = AtomicBool::new(false);
    let stop = || ending.load(Ordering::Relaxed);
    let error = std::thread::scope(|scope| {
        let player = spawn_scoped_traced(scope, || {
            attached(&session, &[]);
            ending.store(true, Ordering::Relaxed);
        });
        let fd = session.as_fd();
        let machine = RemoteMachine::attach(ours, (fd, fd), &stop);
        player.join().unwrap();
        machine.err().unwrap()
    });
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
}
