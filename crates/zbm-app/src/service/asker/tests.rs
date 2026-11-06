#![allow(
    clippy::disallowed_methods,
    reason = "the service's worker and the keyholder run on threads of their own, in real time"
)]

use std::path::Path;
use std::sync::Arc;

use zbm_luks::unlocker::{Luks, Settings};
use zbm_platform::Context;
use zbm_platform::event::{Reply, spawn_scoped_traced};
use zbm_platform::interaction::{Awaited, SecretKind, Subject, Told};
use zbm_platform::secret::Secret;
use zbm_sys::socket::Connection;
use zbm_testkit::fakes::FakeSystem;
use zbm_types::RetryDelay;

use super::*;
use crate::keyholder::UnlockRequest;
use crate::keyholder::tests::Given;
use crate::model::{Outcome, Question};
use crate::service::{MachineService, ServiceHandle};
use crate::{App, Runtime};

/// What the menu sends to open the volume of `/dev/sda2`.
fn open(name: Option<&str>) -> Action {
    Action::Open {
        device: "/dev/sda2".to_owned(),
        name: name.map(|name| name.parse().unwrap()),
    }
}

/// The volume as the keyholder names it.
const VOLUME: VolumeRef<'static> = VolumeRef {
    name: "luks-1111",
    device: "/dev/sda2",
};

/// The run of a keyholder that asks for the passphrase of [`VOLUME`], and
/// opens it when it is `pw`.
fn passphrase(interaction: &dyn Interaction, _: &UnlockRequest) -> Vec<Event> {
    let request = SecretRequest {
        subject: VOLUME.into(),
        kind: SecretKind::Passphrase,
        attempt: 1,
        tries: Some(3),
        opened_elsewhere: None,
        description: None,
    };
    let outcome = match interaction.secret(&request) {
        Asked::Secret(secret) if secret.expose() == b"pw" => Outcome::Done("open".to_owned()),
        _ => Outcome::Failed {
            what: "not open".to_owned(),
            why: Vec::new(),
            next: Vec::new(),
        },
    };
    vec![Event::Finished(outcome)]
}

/// Opens a volume with `action` on a service whose LUKS session asks
/// through an asker, and whose keyholder, on a thread of its own, runs
/// `run`; `drive` takes the service's handle, and says what it saw. The
/// threads must all end for this to return.
fn opening<T>(
    action: Action,
    run: fn(&dyn Interaction, &UnlockRequest) -> Vec<Event>,
    drive: impl FnOnce(ServiceHandle) -> T,
) -> T {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let settings = Settings::new("/zfsbootmenu");
    let asker = Asker::new(&fake.clock);
    let (client, keyholder) = Connection::pair().unwrap();
    let keyholders = Given::new(client);
    let luks = Luks {
        context: fake.context(),
        settings: &settings,
        interaction: &asker,
    };
    let app = App::new(Context::new(fake.system()), &runtime)
        .with_keyholders(&keyholders)
        .with_luks(luks);
    let service = MachineService::new(app, fake.system()).with_asker(&asker);
    std::thread::scope(|scope| {
        spawn_scoped_traced(scope, || {
            let _ = crate::keyholder::serve(&keyholder, &fake.runner, run, |_| false);
        });
        let mut handle = service.start(scope).unwrap();
        handle.send(action).unwrap();
        drive(handle)
    })
}

/// The first question the service's action asks.
fn asked(handle: &mut ServiceHandle) -> (Event, Reply<Action>) {
    loop {
        for report in handle.take() {
            if let Report::Ask { event, reply } = report {
                return (*event, reply);
            }
        }
        std::thread::sleep(POLL);
    }
}

/// What comes back until the action is done, a withdrawal as
/// [`Event::Withdrawn`].
fn until_done(handle: &mut ServiceHandle) -> Vec<Event> {
    let mut events = Vec::new();
    while handle.is_busy() {
        std::thread::sleep(Duration::from_millis(1));
        for report in handle.take() {
            match report {
                Report::Event(event) => events.push(*event),
                Report::Withdraw => events.push(Event::Withdrawn),
                Report::Ask { .. } | Report::Done => {}
            }
        }
    }
    events
}

fn unlock(secret: &[u8]) -> Action {
    Action::Unlock {
        volume: "/dev/sda2".to_owned(),
        method: Method::Passphrase,
        secret: Some(Arc::new(Secret::copy_from(secret))),
    }
}

#[test]
fn a_keyholder_s_question_reaches_the_menu_and_its_answer_goes_to_the_reply() {
    let (event, ended) = opening(open(None), passphrase, |mut handle| {
        let (event, reply) = asked(&mut handle);
        // The answer goes past the queue, which the action holds.
        reply.send(unlock(b"pw"));
        (event, until_done(&mut handle))
    });

    // The volume as the menu names it: by its device, as none was given.
    assert_eq!(
        event,
        Event::UnlockRequested {
            volume: "/dev/sda2".to_owned(),
            method: Method::Passphrase,
            tries_left: Some(3),
            notice: None,
            wait: None,
        }
    );
    assert_eq!(
        ended.last(),
        Some(&Event::Finished(Outcome::Done("open".to_owned())))
    );
}

#[test]
fn the_dialog_names_the_volume_as_the_menu_asked_to_open_it() {
    let (event, _) = opening(open(Some("root")), passphrase, |mut handle| {
        let (event, reply) = asked(&mut handle);
        reply.send(Action::SkipUnlock("root".to_owned()));
        (event, until_done(&mut handle))
    });

    assert!(
        matches!(&event, Event::UnlockRequested { volume, .. } if volume == "root"),
        "{event:?}"
    );
}

#[test]
fn cancelling_the_action_while_it_asks_withdraws_the_question() {
    let ended = opening(open(None), passphrase, |mut handle| {
        // The reply stays, unanswered: the cancel alone ends the wait.
        let (_, _reply) = asked(&mut handle);
        handle.cancel();
        until_done(&mut handle)
    });

    assert!(ended.contains(&Event::Withdrawn), "{ended:?}");
    assert_eq!(ended.last(), Some(&Event::Finished(Outcome::Cancelled)));
}

#[test]
fn once_the_action_is_cancelled_an_answer_is_none_and_nothing_more_is_asked() {
    let fake = FakeSystem::new();
    let asker = Asker::new(&fake.clock);
    let reports = zbm_platform::event::Mailbox::new().unwrap();
    asker.attach(reports.sender());
    let token = CancelToken::new().unwrap();
    asker.performing(&open(None), &token);
    let request = SecretRequest {
        subject: VOLUME.into(),
        kind: SecretKind::Passphrase,
        attempt: 1,
        tries: Some(3),
        opened_elsewhere: None,
        description: None,
    };

    // Typed as the action was cancelled: no secret.
    let (secret, reports) = std::thread::scope(|scope| {
        let menu = spawn_scoped_traced(scope, || {
            loop {
                let came = reports.take();
                if let Some(Report::Ask { reply, .. }) = came.into_iter().next() {
                    token.cancel();
                    reply.send(unlock(b"pw"));
                    return reports;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        (asker.secret(&request), menu.join().unwrap())
    });
    assert!(matches!(secret, Asked::Ended));

    // Cancelled, it asks the menu nothing more.
    assert!(matches!(asker.secret(&request), Asked::Ended));
    assert!(reports.take().is_empty());
}

#[test]
fn a_front_end_that_goes_while_a_question_waits_ends_every_thread() {
    // As when another instance takes the menu over: the handle goes, the
    // reply with it, and the scope ends.
    opening(open(None), passphrase, |mut handle| {
        let (_, reply) = asked(&mut handle);
        drop(handle);
        drop(reply);
    });
}

/// The run of a keyholder that asks to retry [`VOLUME`], then waits for
/// its security key, and says what came.
fn retry_then_wait(interaction: &dyn Interaction, _: &UnlockRequest) -> Vec<Event> {
    let retry = PromptRequest {
        subject: Subject::Retry {
            name: VOLUME.name.to_owned(),
            device: VOLUME.device.to_owned(),
        },
        delay: Some(RetryDelay::DEFAULT),
        message: String::new(),
        on_return: String::new(),
        on_escape: String::new(),
    };
    let answered = interaction.ask(&retry);
    interaction.notify(Tell {
        told: Told::Touch,
        text: "Touch the key.",
        highlight: None,
    });
    let waited = interaction.wait(&WaitRequest {
        volume: VOLUME,
        awaited: Awaited::SecurityKey,
        seconds: 30,
        present: &|| false,
    });
    let done = format!("{answered:?} {waited:?}");
    vec![Event::Finished(Outcome::Done(done))]
}

#[test]
fn a_retry_and_a_wait_are_asked_as_the_start_up_view_asks_them() {
    let (questions, ended) = opening(open(None), retry_then_wait, |mut handle| {
        let (retry, reply) = asked(&mut handle);
        reply.send(Action::Answer(Answer::Continue));
        let (wait, reply) = asked(&mut handle);
        reply.send(Action::SkipUnlock("/dev/sda2".to_owned()));
        ((retry, wait), until_done(&mut handle))
    });

    assert_eq!(
        questions.0,
        Event::Asked {
            question: Question::RetryUnlock {
                volume: "luks-1111".to_owned()
            },
            seconds: Some(5),
        }
    );
    assert!(
        matches!(
            questions.1,
            Event::UnlockRequested {
                method: Method::Fido2,
                wait: Some(30),
                ..
            }
        ),
        "{:?}",
        questions.1
    );
    assert_eq!(
        ended.last(),
        Some(&Event::Finished(Outcome::Done(
            "Continue Skipped".to_owned()
        )))
    );
}

#[test]
fn outside_an_action_of_its_service_nothing_answers() {
    let fake = FakeSystem::new();
    let asker = Asker::new(&fake.clock);
    let request = SecretRequest {
        subject: VOLUME.into(),
        kind: SecretKind::Passphrase,
        attempt: 1,
        tries: None,
        opened_elsewhere: None,
        description: None,
    };
    let retry = PromptRequest {
        subject: Subject::Missing("/dev/sdz".to_owned()),
        delay: None,
        message: String::new(),
        on_return: String::new(),
        on_escape: String::new(),
    };
    let failed_boot = PromptRequest {
        subject: Subject::KexecFailed {
            kernel: "/boot/vmlinuz".to_owned(),
        },
        ..retry.clone()
    };
    let zfs_key = SecretRequest {
        subject: SecretSubject::EncryptionRoot("tank"),
        kind: SecretKind::Passphrase,
        ..request
    };

    assert!(matches!(asker.secret(&request), Asked::Ended));
    assert!(matches!(asker.secret(&zfs_key), Asked::Ended));
    assert_eq!(asker.ask(&retry), Answer::Escape);
    assert_eq!(asker.ask(&failed_boot), Answer::Escape);
    asker.show(Event::Progress("nobody sees this".to_owned()));
    assert_eq!(Asks::list(&asker, VOLUME, Method::Passphrase), "luks-1111");
}
