#![allow(
    clippy::disallowed_methods,
    reason = "the service's worker and the session run on threads of their own, in real time"
)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use pretty_assertions::assert_eq;
use zbm_platform::Context;
use zbm_platform::event::spawn_scoped_traced;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::model::Outcome;
use crate::{App, Runtime};

const WAIT: Option<Duration> = Some(Duration::from_secs(5));

/// A machine whose pool `tank` is writable, so that a snapshot is taken
/// without the console.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    fake
}

/// The snapshot the menu asks for.
fn take() -> Action {
    Action::CreateSnapshot("tank/ROOT/be@new".parse().unwrap())
}

/// What the session sends next.
fn next(menu: &Connection) -> ToMenu {
    ToMenu::receive(menu, WAIT).unwrap().unwrap()
}

/// What the session sends until an action is done, and its last event.
fn until_done(menu: &Connection) -> Vec<ToMenu> {
    let mut messages = Vec::new();
    loop {
        let message = next(menu);
        let done = message == ToMenu::Done;
        messages.push(message);
        if done {
            return messages;
        }
    }
}

/// Serves the menu at the other end of `menu` on `fake`, performing the
/// actions that take the console with `console`, while `client` runs;
/// how serving ended, and what `client` returned.
fn served<T>(
    fake: &FakeSystem,
    console: &(dyn Fn(&Action) -> Vec<Event> + Sync),
    stop: &(dyn Fn() -> bool + Sync),
    client: impl FnOnce(&Connection) -> T,
) -> (io::Result<()>, T) {
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let app = App::new(Context::new(fake.system()), &runtime);
    let served = Served {
        service: MachineService::new(app, fake.system()),
        console,
        stop,
    };
    let (menu, session) = Connection::pair().unwrap();
    std::thread::scope(|scope| {
        let serving = spawn_scoped_traced(scope, || serve(&session, &served));
        let result = client(&menu);
        drop(menu);
        (serving.join().unwrap(), result)
    })
}

/// Whether `message` says the machine changed.
fn is_changed(message: &ToMenu) -> bool {
    matches!(message, ToMenu::Event(event) if matches!(**event, Event::Changed(_)))
}

fn no_console(_: &Action) -> Vec<Event> {
    Vec::new()
}

fn never() -> bool {
    false
}

#[test]
fn a_menu_attaches_with_its_terminal_alone() {
    let (menu, session) = Connection::pair().unwrap();
    let (input, output) = Connection::pair().unwrap();
    ToSession::Attach
        .send(&menu, &[input.as_fd(), output.as_fd()])
        .unwrap();
    attach(&session).unwrap();
    assert_eq!(next(&menu), ToMenu::Attached);

    // Without the terminal, or something else first.
    ToSession::Attach.send(&menu, &[input.as_fd()]).unwrap();
    assert!(attach(&session).is_err());
    assert!(matches!(next(&menu), ToMenu::Refused(_)));
    ToSession::Read.send(&menu, &[]).unwrap();
    assert!(attach(&session).is_err());
    assert!(matches!(next(&menu), ToMenu::Refused(_)));
}

#[test]
fn the_menu_reads_the_machine_and_performs_an_action_meanwhile() {
    let fake = machine();
    let (ended, messages) = served(&fake, &no_console, &never, |menu| {
        ToSession::Read.send(menu, &[]).unwrap();
        let snapshot = next(menu);
        ToSession::Send { action: take() }.send(menu, &[]).unwrap();
        let messages = until_done(menu);
        ToSession::Detach.send(menu, &[]).unwrap();
        (snapshot, messages)
    });
    ended.unwrap();
    let (snapshot, messages) = messages;
    assert!(
        matches!(snapshot, ToMenu::Snapshot(Some(_))),
        "{snapshot:?}"
    );
    assert!(messages.first().is_some_and(is_changed), "{messages:?}");
    assert!(
        messages.iter().any(|message| matches!(
            message,
            ToMenu::Event(event) if matches!(**event, Event::Finished(Outcome::Done(_)))
        )),
        "{messages:?}"
    );
    assert!(
        fake.runner
            .lines()
            .contains(&"zfs snapshot tank/ROOT/be@new".to_owned())
    );
}

#[test]
fn an_action_that_takes_the_console_runs_on_the_terminal_once_none_runs() {
    let fake = machine();
    // The snapshot waits until the test lets it go.
    let held = Arc::new(AtomicBool::new(true));
    let holding = Arc::clone(&held);
    fake.runner.first(move |command| {
        while command.display().starts_with("zfs snapshot") && holding.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    });
    let consoles = AtomicUsize::new(0);
    let console = |action: &Action| {
        assert_eq!(*action, Action::Shell);
        consoles.fetch_add(1, Ordering::Relaxed);
        vec![Event::Finished(Outcome::Cancelled)]
    };
    let (ended, (refused, done)) = served(&fake, &console, &never, |menu| {
        ToSession::Send { action: take() }.send(menu, &[]).unwrap();
        ToSession::Console {
            action: Action::Shell,
        }
        .send(menu, &[])
        .unwrap();
        // Refused while the snapshot is taken.
        let refused = next(menu);
        ToSession::Attach.send(menu, &[]).unwrap();
        assert_eq!(next(menu), ToMenu::Refused("already attached".to_owned()));
        held.store(false, Ordering::Relaxed);
        until_done(menu);
        ToSession::Console {
            action: Action::Shell,
        }
        .send(menu, &[])
        .unwrap();
        (refused, until_done(menu))
    });
    ended.unwrap();
    assert!(matches!(refused, ToMenu::Refused(_)), "{refused:?}");
    assert_eq!(consoles.load(Ordering::Relaxed), 1);
    assert!(done.first().is_some_and(is_changed), "{done:?}");
    assert_eq!(
        done[done.len() - 2..],
        [
            ToMenu::Event(Box::new(Event::Finished(Outcome::Cancelled))),
            ToMenu::Done
        ]
    );
}

#[test]
fn a_menu_that_is_taken_over_is_told_and_one_that_goes_ends_its_service() {
    let fake = machine();
    let stop = || true;
    let (ended, told) = served(&fake, &no_console, &stop, next);
    ended.unwrap();
    assert_eq!(told, ToMenu::Refused(TAKEN_OVER.to_owned()));

    // The menu goes mid-action: what it sent is cancelled.
    let (ended, ()) = served(&fake, &no_console, &never, |menu| {
        ToSession::Send { action: take() }.send(menu, &[]).unwrap();
        ToSession::Cancel.send(menu, &[]).unwrap();
    });
    ended.unwrap();

    // A menu that sends none of the protocol.
    let (ended, ()) = served(&fake, &no_console, &never, |menu| {
        menu.send(b".{\"version\":9}").unwrap();
    });
    assert_eq!(ended.unwrap_err().kind(), io::ErrorKind::InvalidData);
}
