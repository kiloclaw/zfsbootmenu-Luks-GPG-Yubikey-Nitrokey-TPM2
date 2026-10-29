#![allow(
    clippy::disallowed_methods,
    reason = "the service's worker runs on its own thread, in real time"
)]

use std::path::Path;
use std::time::Duration;

use zbm_platform::Context;
use zbm_platform::exec::{Output, Status};
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::BootSettings;

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

/// Collects what `handle` sends back until the action it performs is done.
fn until_done(handle: &mut ServiceHandle) -> Vec<Event> {
    let mut events = Vec::new();
    while handle.is_busy() {
        std::thread::sleep(Duration::from_millis(1));
        for report in handle.take() {
            if let Report::Event(event) = report {
                events.push(*event);
            }
        }
    }
    events
}

#[test]
fn the_worker_performs_an_action_and_reads_the_machine_again() {
    let fake = machine();
    let settings = BootSettings::default();
    let app = App::new(Context::new(fake.system()))
        .with_base(Path::new("/zfsbootmenu"))
        .with_boot(&settings);
    let service = MachineService::new(app, fake.system());

    let events = std::thread::scope(|scope| {
        let mut handle = service.start(scope).unwrap();
        assert!(!handle.is_busy());
        handle.send(take()).unwrap();
        assert!(handle.is_busy());
        until_done(&mut handle)
    });

    assert!(
        matches!(events.first(), Some(Event::Changed(_))),
        "{events:?}"
    );
    assert!(
        matches!(events.last(), Some(Event::Finished(Outcome::Done(_)))),
        "{events:?}"
    );
    assert!(
        fake.runner
            .lines()
            .contains(&"zfs snapshot tank/ROOT/be@new".to_owned())
    );
}

#[test]
fn a_cancelled_action_kills_its_program_and_ends_as_cancelled() {
    let fake = machine();
    // `zfs snapshot` runs until it is killed, as a hung program would.
    fake.runner.first(|command| {
        if command.display() != "zfs snapshot tank/ROOT/be@new" {
            return None;
        }
        let token = command.get_cancel()?.clone();
        while !token.is_cancelled() {
            std::thread::sleep(Duration::from_millis(1));
        }
        Some(Ok(Output::new(Status::from_raw(9), Vec::new())))
    });
    let settings = BootSettings::default();
    let app = App::new(Context::new(fake.system()))
        .with_base(Path::new("/zfsbootmenu"))
        .with_boot(&settings);
    let service = MachineService::new(app, fake.system());

    let events = std::thread::scope(|scope| {
        let mut handle = service.start(scope).unwrap();
        handle.send(take()).unwrap();
        // A second action, cancelled before it started: it runs nothing.
        handle
            .send(Action::SetDefaultEnvironment(
                "tank/ROOT/be".parse().unwrap(),
            ))
            .unwrap();
        while !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("zfs snapshot"))
        {
            std::thread::yield_now();
        }
        handle.cancel();
        until_done(&mut handle)
    });

    let finished: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event, Event::Finished(_)))
        .collect();
    assert_eq!(
        finished,
        [
            &Event::Finished(Outcome::Cancelled),
            &Event::Finished(Outcome::Cancelled)
        ]
    );
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("zpool set bootfs")),
        "{:?}",
        fake.runner.lines()
    );
}

#[test]
fn an_action_performed_in_place_reads_the_machine_first() {
    let fake = machine();
    let service = MachineService::new(App::new(Context::new(fake.system())), fake.system());
    let token = CancelToken::new().unwrap();
    token.cancel();

    let events = service.perform(&take(), &token);

    assert!(matches!(
        events.as_slice(),
        [Event::Changed(_), Event::Finished(Outcome::Cancelled)]
    ));
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("zfs snapshot"))
    );
}

#[test]
fn the_worker_ends_when_the_front_end_is_gone_while_it_works() {
    let fake = machine();
    let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = started.clone();
    // The snapshot waits until the front end is gone.
    fake.runner.first(move |command| {
        if command.display() == "zfs snapshot tank/ROOT/be@new" {
            seen.store(true, std::sync::atomic::Ordering::Release);
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    });
    let settings = BootSettings::default();
    let app = App::new(Context::new(fake.system()))
        .with_base(Path::new("/zfsbootmenu"))
        .with_boot(&settings);
    let service = MachineService::new(app, fake.system());

    std::thread::scope(|scope| {
        let mut handle = service.start(scope).unwrap();
        assert!(zbm_platform::event::Mailbox::<()>::new().is_ok());
        let _readable = handle.as_fd();
        handle.send(take()).unwrap();
        while !started.load(std::sync::atomic::Ordering::Acquire) {
            std::thread::yield_now();
        }
        drop(handle);
    });
}
