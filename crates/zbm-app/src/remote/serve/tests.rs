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
fn a_remote_menu_scans_the_pools_and_gets_them_without_a_snapshot() {
    let fake = machine();
    let (ended, messages) = served(&fake, &no_console, &never, |menu| {
        ToSession::Send {
            action: Action::ScanPools,
        }
        .send(menu, &[])
        .unwrap();
        let messages = until_done(menu);
        ToSession::Detach.send(menu, &[]).unwrap();
        messages
    });
    ended.unwrap();
    assert_eq!(
        messages,
        [
            ToMenu::Event(Box::new(Event::PoolsScanned(Vec::new()))),
            ToMenu::Done
        ]
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
        // Refused while the snapshot is taken: it failed.
        let refused = until_done(menu);
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
    assert!(
        matches!(
            refused.as_slice(),
            [ToMenu::Event(event), ToMenu::Done]
                if matches!(**event, Event::Finished(Outcome::Failed { .. }))
        ),
        "{refused:?}"
    );
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

/// Serves the menu at the other end of `menu` with a LUKS session whose
/// person is over SSH (`remote`) or not, its keyholder on a thread of its
/// own, the volumes of `crate::volumes::tests::machine`, while `client`
/// runs; what `client` returned, and how the volumes were opened.
fn served_with_volumes<T>(remote: bool, client: impl FnOnce(&Connection) -> T) -> (T, String) {
    use zbm_luks::device::OpenCheck;
    use zbm_luks::unlocker::{Luks, Presence, Settings};

    let fake = crate::volumes::tests::machine();
    fake.files.dir("/zfsbootmenu");
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let mut settings = Settings::new("/zfsbootmenu");
    settings.open_check = OpenCheck::Markers("/fake/holders".into());
    if remote {
        settings.presence = Presence::Remote;
    }
    let asker = crate::service::asker::Asker::new(&fake.clock);
    let (to_keyholder, keyholder) = Connection::pair().unwrap();
    let keyholders = crate::keyholder::tests::Given::new(to_keyholder);
    let luks = Luks {
        context: fake.context(),
        settings: &settings,
        interaction: &asker,
    };
    let app = App::new(Context::new(fake.system()), &runtime)
        .with_keyholders(&keyholders)
        .with_luks(luks);
    let served = Served {
        service: MachineService::new(app, fake.system()).with_asker(&asker),
        console: &no_console,
        stop: &never,
    };
    let (menu, session) = Connection::pair().unwrap();
    let result = std::thread::scope(|scope| {
        // The keyholder opens the volume as its client asked, as
        // `zbm unlock serve` does.
        spawn_scoped_traced(scope, || {
            let run = |interaction: &dyn zbm_platform::interaction::Interaction,
                       request: &crate::keyholder::UnlockRequest| {
                let mut settings = settings.clone();
                if request.remote {
                    settings.presence = Presence::Remote;
                }
                let luks = Luks {
                    context: fake.context(),
                    settings: &settings,
                    interaction,
                };
                crate::Volumes::new(luks).apply(&request.action)
            };
            let _ = crate::keyholder::serve(&keyholder, &fake.runner, run, |_| false);
        });
        let serving = spawn_scoped_traced(scope, || serve(&session, &served));
        let result = client(&menu);
        drop(menu);
        serving.join().unwrap().unwrap();
        // A keyholder never started ends with its client.
        drop(crate::keyholder::Keyholders::start(&keyholders));
        result
    });
    let unlocked = fake
        .files
        .text("/zfsbootmenu/luks-unlocked")
        .unwrap_or_default();
    (result, unlocked)
}

/// The volume the menu opens, as its snapshot lists it.
fn open_locked() -> Action {
    Action::Open {
        device: "/dev/sdb2".to_owned(),
        name: None,
    }
}

/// Opens [`open_locked`] with the passphrase `pw`, answered as the menu
/// answers; what the session sent until the action was done.
fn open_with_passphrase(menu: &Connection) -> Vec<ToMenu> {
    ToSession::Send {
        action: open_locked(),
    }
    .send(menu, &[])
    .unwrap();
    let mut messages = Vec::new();
    let question = loop {
        let message = next(menu);
        let asked = match &message {
            ToMenu::Ask { question, event } if matches!(**event, Event::UnlockRequested { .. }) => {
                Some(*question)
            }
            _ => None,
        };
        messages.push(message);
        if let Some(question) = asked {
            break question;
        }
    };
    let answer = |secret: &[u8]| Action::Unlock {
        volume: "/dev/sdb2".to_owned(),
        method: crate::model::Method::Passphrase,
        secret: Some(Arc::new(zbm_platform::Secret::copy_from(secret))),
    };
    // An answer to another question, as one withdrawn meanwhile, goes to
    // nobody: the question asked waits on.
    let stale = ToSession::Answer {
        question: question + 1,
        action: answer(b"wrong"),
    };
    stale.send(menu, &[]).unwrap();
    let action = answer(b"pw");
    ToSession::Answer { question, action }
        .send(menu, &[])
        .unwrap();
    messages.extend(until_done(menu));
    ToSession::Detach.send(menu, &[]).unwrap();
    messages
}

#[test]
fn a_passphrase_typed_over_ssh_opens_the_volume_but_not_the_lock() {
    for remote in [true, false] {
        let (messages, unlocked) = served_with_volumes(remote, open_with_passphrase);

        assert!(
            messages.iter().any(|message| matches!(
                message,
                ToMenu::Event(event) if matches!(**event, Event::Finished(Outcome::Done(_)))
            )),
            "{messages:?}"
        );
        // Over SSH it is recorded as remote: the locked menu's gate and the
        // recovery shell stay shut; at the machine, it is attended.
        let how = if remote { "remote" } else { "attended" };
        assert!(unlocked.ends_with(&format!("\t{how}\n")), "{unlocked:?}");
        let files = zbm_testkit::fakes::FakeFiles::new();
        files.file("/zfsbootmenu/luks-unlocked", &unlocked);
        let state = zbm_state::State::new(&files, "/zfsbootmenu");
        assert_eq!(state.has_attended_unlock(), !remote);
    }
}

#[test]
fn the_session_opens_only_a_volume_its_snapshot_lists_and_one_at_a_time() {
    let refused = |action: Action| {
        let (messages, _) = served_with_volumes(true, |menu| {
            ToSession::Send { action }.send(menu, &[]).unwrap();
            let messages = until_done(menu);
            ToSession::Detach.send(menu, &[]).unwrap();
            messages
        });
        match messages.as_slice() {
            [ToMenu::Event(event), ToMenu::Done] => match &**event {
                Event::Finished(Outcome::Failed { why, .. }) => why.join(" "),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    };
    let open = |device: &str, name: Option<&str>| Action::Open {
        device: device.to_owned(),
        name: name.map(|name| name.parse().unwrap()),
    };

    assert_eq!(
        refused(open("/dev/sdz9", None)),
        "the machine has no such volume"
    );
    // Listed by its device, so opened under no other name.
    assert_eq!(
        refused(open("/dev/sdb2", Some("evil"))),
        "the machine has no such volume"
    );
    assert_eq!(
        refused(Action::UnlockAll),
        "the menu opens one volume at a time"
    );
    assert_eq!(
        refused(Action::SkipUnlock("/dev/sdb2".to_owned())),
        "an answer goes with the question it answers"
    );
}

#[test]
fn an_answer_to_nothing_asked_goes_to_nobody() {
    let fake = machine();
    let (ended, read) = served(&fake, &no_console, &never, |menu| {
        let answer = ToSession::Answer {
            question: 1,
            action: Action::Answer(zbm_types::Answer::Continue),
        };
        answer.send(menu, &[]).unwrap();
        // Nothing comes of it: the next message answers the next request.
        ToSession::Read.send(menu, &[]).unwrap();
        let read = next(menu);
        ToSession::Detach.send(menu, &[]).unwrap();
        read
    });
    ended.unwrap();
    assert!(matches!(read, ToMenu::Snapshot(_)), "{read:?}");
}

#[test]
fn an_answer_reaches_only_the_question_asked_now() {
    use zbm_platform::event::{NoAnswer, reply};

    let mut asked = Asked::default();
    let (first, _first_awaiting) = reply();
    assert_eq!(asked.ask(first), 1);
    // The first is withdrawn as the device came; the next is asked at once.
    asked.withdraw();
    let (second, awaiting) = reply();
    assert_eq!(asked.ask(second), 2);

    // The answer the menu sent to the first goes to nobody.
    asked.answer(1, Action::Answer(zbm_types::Answer::Continue));
    assert!(matches!(
        awaiting.wait(Some(Duration::ZERO)),
        Err(NoAnswer::NotYet)
    ));
    // Not an answer: nobody.
    asked.answer(2, Action::UnlockAll);
    assert!(matches!(
        awaiting.wait(Some(Duration::ZERO)),
        Err(NoAnswer::NotYet)
    ));
    asked.answer(2, Action::SkipUnlock("/dev/sdb2".to_owned()));
    assert!(matches!(
        awaiting.wait(Some(Duration::ZERO)),
        Ok(Action::SkipUnlock(_))
    ));
}

/// What the session answers a remote menu that sends `action`.
fn answered(fake: &FakeSystem, action: Action) -> Vec<ToMenu> {
    let (ended, messages) = served(fake, &no_console, &never, |menu| {
        ToSession::Send { action }.send(menu, &[]).unwrap();
        let messages = until_done(menu);
        ToSession::Detach.send(menu, &[]).unwrap();
        messages
    });
    ended.unwrap();
    messages
}

#[test]
fn a_remote_menu_reads_the_messages_of_a_pools_disk_alone() {
    let fake = machine();
    fake.runner.respond("zfs version -j", 1, "");
    fake.runner.respond(
        "zpool list -H -p -o name,health,readonly,bootfs,guid",
        0,
        "tank\tONLINE\toff\t-\t7\n",
    );
    fake.runner.respond(
        "zpool status -p -P",
        0,
        "  pool: tank\n state: ONLINE\nconfig:\n\n\tNAME STATE READ WRITE CKSUM\n\
         \ttank ONLINE 0 0 0\n\t  /dev/sda2 ONLINE 0 0 0\n\nerrors: No known data errors\n",
    );
    let device = |name: &str| Action::LoadDeviceLog(zbm_types::Untrusted::new(name.to_owned()));

    // Any other device, or a path the kernel could read, is refused.
    for refused in ["/dev/sdb", "../../etc/shadow"] {
        let messages = answered(&fake, device(refused));
        let [ToMenu::Event(event), ToMenu::Done] = messages.as_slice() else {
            panic!("{messages:?}");
        };
        let Event::Finished(Outcome::Failed { why, .. }) = &**event else {
            panic!("{event:?}");
        };
        assert_eq!(why, &["no pool of the machine has such a device"]);
    }
    // A disk of a pool: its messages, and no snapshot after them.
    let messages = answered(&fake, device("/dev/sda2"));
    let [ToMenu::Event(event), ToMenu::Done] = messages.as_slice() else {
        panic!("{messages:?}");
    };
    assert!(
        matches!(&**event, Event::DeviceLogLoaded(log) if log.device.get() == "/dev/sda2"),
        "{event:?}"
    );
    // An export is a change, which the session refuses no more than another.
    assert_eq!(
        refusal_of(&fake, &Action::Export("tank".parse().unwrap())),
        None
    );
}

/// Why the session refuses `action` on `fake`, if it does.
fn refusal_of(fake: &FakeSystem, action: &Action) -> Option<String> {
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let app = App::new(Context::new(fake.system()), &runtime);
    let served = Served {
        service: MachineService::new(app, fake.system()),
        console: &no_console,
        stop: &never,
    };
    refusal(&served, action)
}

#[test]
fn a_remote_menu_loads_the_snapshots_of_a_boot_environment_alone() {
    let fake = machine();
    fake.runner.respond("zfs version -j", 1, "");
    let listing = |name: &str| {
        format!("zfs list -H -p -t filesystem -o name,mountpoint,canmount,keystatus -r {name}")
    };
    fake.runner.respond(
        &listing("tank/ROOT/be"),
        0,
        "tank/ROOT/be\t/\tnoauto\t-\ntank/ROOT/be/home\t/home\ton\t-\n",
    );
    fake.runner
        .respond(&listing("tank/ROOT"), 0, "tank/ROOT\tnone\ton\t-\n");
    fake.runner.respond(
        "zfs list -H -p -t snapshot -o name,creation,used -d 1 -s creation tank/ROOT/be",
        0,
        "tank/ROOT/be@old\t100\t4096\ntank/ROOT/be@new\t200\t0\n",
    );
    let load = |name: &str| Action::LoadSnapshots(name.parse().unwrap());

    // A dataset that is no boot environment, or that does not exist.
    for refused in ["tank/ROOT", "tank/ROOT/be/home", "tank/gone"] {
        assert_eq!(
            refusal_of(&fake, &load(refused)).as_deref(),
            Some("the machine has no such boot environment"),
            "{refused}"
        );
    }
    // A boot environment: its snapshots, newest first, and no snapshot of
    // the machine after them.
    let messages = answered(&fake, load("tank/ROOT/be"));
    let [ToMenu::Event(event), ToMenu::Done] = messages.as_slice() else {
        panic!("{messages:?}");
    };
    let Event::SnapshotsLoaded(list) = &**event else {
        panic!("{event:?}");
    };
    let names: Vec<&str> = list.snapshots.iter().map(|s| s.name.name()).collect();
    assert_eq!(names, ["new", "old"]);
}

mod history;
