use std::sync::{Mutex, PoisonError};

use pretty_assertions::assert_eq;
use zbm_platform::interaction::{
    Answer, Asked, Awaited, PromptRequest, SecretKind, Subject, Told, Wait,
};
use zbm_platform::{Runner, Secret};
use zbm_testkit::FakeRunner;
use zbm_types::RetryDelay;

use super::*;
use crate::model::{Outcome, UnlockOutcome};

/// Keyholders that hand out one connection made beforehand, and then none.
pub(crate) struct Given(Mutex<Option<Connection>>);

impl Given {
    pub(crate) fn new(connection: Connection) -> Given {
        Given(Mutex::new(Some(connection)))
    }

    pub(crate) fn none() -> Given {
        Given(Mutex::new(None))
    }
}

struct Held(Connection);

impl Keyholder for Held {
    fn connection(&self) -> &Connection {
        &self.0
    }
}

impl Keyholders for Given {
    fn start(&self) -> io::Result<Box<dyn Keyholder + '_>> {
        let given = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        given
            .map(|connection| Box::new(Held(connection)) as Box<dyn Keyholder>)
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }
}

fn unlock_all() -> UnlockRequest {
    UnlockRequest {
        action: Action::UnlockAll,
        retry: false,
    }
}

#[test]
fn every_message_to_the_keyholder_reads_back_as_it_was_sent() {
    let to = [
        ToKeyholder::Unlock(unlock_all()),
        ToKeyholder::Unlock(UnlockRequest {
            action: Action::Open {
                device: "UUID=1111".to_owned(),
                name: Some("root".parse().unwrap()),
            },
            retry: true,
        }),
        ToKeyholder::Unlock(UnlockRequest {
            action: Action::Open {
                device: "/dev/sda2".to_owned(),
                name: None,
            },
            retry: false,
        }),
        ToKeyholder::Unlock(UnlockRequest {
            action: Action::OpenWithKeyFile {
                device: "/dev/sda2".parse().unwrap(),
                name: "v".parse().unwrap(),
                key_file: "/k".into(),
                options: zbm_crypt::OpenOptions {
                    allow_discards: true,
                    read_only: false,
                },
            },
            retry: false,
        }),
        ToKeyholder::Answer(Answer::Continue),
        ToKeyholder::Answer(Answer::Escape),
        ToKeyholder::Secret { length: 9 },
        ToKeyholder::Elsewhere,
        ToKeyholder::Ended,
        ToKeyholder::Waited(Wait::Present),
        ToKeyholder::Waited(Wait::Skipped),
        ToKeyholder::Waited(Wait::TimedOut),
        ToKeyholder::Leased { console: true },
        ToKeyholder::Query(Query::OpenedElsewhere),
        ToKeyholder::Query(Query::Present),
    ];
    for message in to {
        let bytes = message.encode().unwrap();
        assert_eq!(ToKeyholder::decode(&bytes), Ok(message));
    }
}

/// The keyholder's requests for each secret and each wait.
fn requests() -> Vec<FromKeyholder> {
    let secret = |kind| {
        FromKeyholder::Secret(SecretAsk {
            name: "root".to_owned(),
            device: "/dev/sda2".to_owned(),
            kind,
            attempt: 2,
            tries: Some(3),
            description: Some("/dev/sda2 (data, 8 GiB)".to_owned()),
            may_open_elsewhere: true,
        })
    };
    let wait = |awaited| {
        FromKeyholder::Wait(WaitAsk {
            name: "root".to_owned(),
            device: "/dev/sda2".to_owned(),
            awaited,
            seconds: 30,
        })
    };
    let mut from = Vec::new();
    from.extend(
        [
            SecretKind::Passphrase,
            SecretKind::Tpm2Pin,
            SecretKind::Fido2Pin,
            SecretKind::YubiKeyPassphrase("2".to_owned()),
            SecretKind::KeyFilePassphrase("/k.gpg".to_owned()),
            SecretKind::SecretKeyPassphrase("0xAB".to_owned()),
            SecretKind::CardPin {
                serial: "D276".to_owned(),
                left: "3".to_owned(),
            },
        ]
        .map(secret),
    );
    from.extend(
        [
            Awaited::SecurityKey,
            Awaited::OpenPgpKey,
            Awaited::YubiKey,
            Awaited::KeyDevice {
                spec: "LABEL=keys".to_owned(),
                file: "/root.key".to_owned(),
            },
        ]
        .map(wait),
    );
    from
}

#[test]
fn every_message_of_the_keyholder_reads_back_as_it_was_sent() {
    let mut from = vec![
        FromKeyholder::Ask(PromptRequest {
            subject: Subject::Retry {
                name: "root".to_owned(),
                device: "/dev/sda2".to_owned(),
            },
            delay: RetryDelay::new(7),
            message: "not opened".to_owned(),
            on_return: "retry".to_owned(),
            on_escape: "skip".to_owned(),
        }),
        FromKeyholder::Ask(PromptRequest {
            subject: Subject::Missing("LABEL=keys".to_owned()),
            delay: None,
            message: "missing".to_owned(),
            on_return: "look again".to_owned(),
            on_escape: "skip".to_owned(),
        }),
        FromKeyholder::Lease,
        FromKeyholder::Returned,
        FromKeyholder::Answered(true),
        FromKeyholder::Finished(vec![
            Event::UnlockFinished {
                volume: "a".to_owned(),
                outcome: UnlockOutcome::Opened,
            },
            Event::UnlockFinished {
                volume: "b".to_owned(),
                outcome: UnlockOutcome::Skipped,
            },
            Event::UnlockFinished {
                volume: "c".to_owned(),
                outcome: UnlockOutcome::Failed("wrong key".to_owned()),
            },
            Event::Finished(Outcome::Done("unlocked".to_owned())),
        ]),
    ];
    from.extend(requests());
    from.extend(
        [Told::Working, Told::Touch, Told::Refused, Told::Heading].map(|told| {
            FromKeyholder::Notify(Notice {
                told,
                text: "Unlocking /dev/sda2".to_owned(),
                highlight: Some("/dev/sda2".to_owned()),
            })
        }),
    );
    let outcomes = [
        Outcome::Warning("note".to_owned()),
        Outcome::Cancelled,
        Outcome::Failed {
            what: "unable to unlock".to_owned(),
            why: vec!["no key".to_owned()],
            next: vec!["zluks unlock".to_owned()],
        },
    ];
    from.extend(outcomes.map(|outcome| FromKeyholder::Finished(vec![Event::Finished(outcome)])));
    for message in from {
        assert_eq!(FromKeyholder::decode(&message.encode()), Ok(message));
    }
}

#[test]
fn messages_of_another_version_or_none_of_the_protocol_are_refused() {
    let read = |text: &str| ToKeyholder::decode(text.as_bytes());
    assert_eq!(
        read(r#"{"version":2,"message":"ended"}"#),
        Err(Unread::Version(2))
    );
    for text in [
        "",
        "[]",
        r#"{"version":1}"#,
        r#"{"version":1,"message":"shell"}"#,
        r#"{"version":1,"message":"secret","length":70000}"#,
        r#"{"version":1,"message":"unlock","unlock":{"action":"boot"},"retry":false}"#,
        r#"{"version":1,"message":"answer","answer":"maybe"}"#,
    ] {
        assert_eq!(read(text), Err(Unread::Malformed), "{text}");
    }
    let from = |text: &str| FromKeyholder::decode(text.as_bytes());
    let secret = |kind: &str, description: &str| {
        let fields = r#""name":"r","device":"/d","attempt":1,"tries":null"#;
        let flags = r#""may_open_elsewhere":false"#;
        let rest = format!(r#""kind":{kind},"description":{description}"#);
        format!(r#"{{"version":1,"message":"secret",{fields},{flags},{rest}}}"#)
    };
    let finished =
        |event: &str| format!(r#"{{"version":1,"message":"finished","events":[{event}]}}"#);
    for text in [
        finished(r#"{"changed":1}"#),
        finished(r#"{"unlock finished":{"volume":"v","outcome":5}}"#),
        r#"{"version":1,"message":"shout"}"#.to_owned(),
        secret(r#""pin""#, "null"),
        secret(r#"{"x":"y"}"#, "null"),
        secret(r#""passphrase""#, "5"),
        r#"{"version":1,"message":"wait","name":"r","device":"/d","awaited":"cable","seconds":1}"#
            .to_owned(),
    ] {
        assert_eq!(from(&text), Err(Unread::Malformed), "{text}");
    }
    // Only the volumes' actions go to the keyholder.
    let shell = ToKeyholder::Unlock(UnlockRequest {
        action: Action::Shell,
        retry: false,
    });
    assert_eq!(shell.encode(), None);
    assert!(!is_unlocking(&Action::Shell) && !is_unlocking(&Action::Close(None)));
    // An event other than the volumes' is left out.
    let finished = FromKeyholder::Finished(vec![Event::Progress("…".to_owned())]);
    assert_eq!(
        FromKeyholder::decode(&finished.encode()),
        Ok(FromKeyholder::Finished(Vec::new()))
    );
}

/// The person at the console: what each request got, and its answers.
#[derive(Default)]
struct Person {
    seen: Mutex<Vec<String>>,
    runner: FakeRunner,
}

impl Person {
    fn saw(&self, what: String) {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(what);
    }

    fn seen(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Interaction for Person {
    fn ask(&self, request: &PromptRequest) -> Answer {
        self.saw(format!("ask {}", request.message));
        Answer::Continue
    }

    fn secret(&self, request: &SecretRequest<'_>) -> Asked {
        let elsewhere = request.opened_elsewhere.map(|opened| opened());
        self.saw(format!(
            "secret {} {:?} elsewhere {elsewhere:?}",
            request.volume.name, request.kind
        ));
        match request.attempt {
            1 => Asked::Secret(Secret::copy_from(b"correct horse")),
            2 => Asked::Secret(Secret::new()),
            3 => Asked::Elsewhere,
            _ => Asked::Ended,
        }
    }

    fn wait(&self, request: &WaitRequest<'_>) -> Wait {
        self.saw(format!(
            "wait {:?} present {}",
            request.awaited,
            (request.present)()
        ));
        Wait::Skipped
    }

    fn notify(&self, tell: Tell<'_>) {
        self.saw(format!("notify {:?} {}", tell.told, tell.text));
    }

    fn lease_console(&self, run: &mut dyn FnMut(Option<&dyn Runner>)) {
        self.saw("lend".to_owned());
        run(Some(&self.runner));
        self.saw("take back".to_owned());
    }
}

/// What the keyholder asks in the runs of these tests, and what it got.
fn ask_everything(interaction: &dyn Interaction, console: &FakeRunner) -> Vec<String> {
    let mut got = Vec::new();
    interaction.notify(Tell::new(Told::Working, "Unlocking /dev/sda2"));
    let prompt = PromptRequest {
        subject: Subject::Missing("LABEL=keys".to_owned()),
        delay: None,
        message: "missing".to_owned(),
        on_return: "look again".to_owned(),
        on_escape: "skip".to_owned(),
    };
    got.push(format!("{:?}", interaction.ask(&prompt)));
    let opened = || true;
    for attempt in 1..=4 {
        let request = SecretRequest {
            volume: VolumeRef {
                name: "root",
                device: "/dev/sda2",
            },
            kind: SecretKind::Passphrase,
            attempt,
            tries: None,
            opened_elsewhere: (attempt == 1).then_some(&opened as &dyn Fn() -> bool),
            description: None,
        };
        got.push(match interaction.secret(&request) {
            Asked::Secret(secret) => {
                format!("secret {:?}", String::from_utf8_lossy(secret.expose()))
            }
            other => format!("{other:?}"),
        });
    }
    let waited = interaction.wait(&WaitRequest {
        volume: VolumeRef {
            name: "root",
            device: "/dev/sda2",
        },
        awaited: Awaited::YubiKey,
        seconds: 5,
        present: &|| false,
    });
    got.push(format!("{waited:?}"));
    interaction.lease_console(&mut |runner| {
        let own = runner.is_some_and(|runner| std::ptr::addr_eq(runner, console));
        got.push(format!("leased, the keyholder's terminal: {own}"));
    });
    got
}

#[test]
fn a_run_asks_the_client_and_ends_with_its_events() {
    let (client, keyholder) = Connection::pair().unwrap();
    let person = Person::default();
    let console = FakeRunner::new();

    let (events, got) = std::thread::scope(|scope| {
        let served = zbm_platform::event::spawn_scoped_traced(scope, || {
            let mut got = Vec::new();
            serve(&keyholder, &console, |interaction, request| {
                got = ask_everything(interaction, &console);
                got.push(format!("{:?}", request.action));
                vec![Event::Finished(Outcome::Done("unlocked".to_owned()))]
            })
            .unwrap();
            got
        });
        let events = unlock(&Given::new(client), &person, &unlock_all()).unwrap();
        (events, served.join().unwrap())
    });

    assert_eq!(
        events,
        [Event::Finished(Outcome::Done("unlocked".to_owned()))]
    );
    assert_eq!(
        got,
        [
            "Continue",
            "secret \"correct horse\"",
            "secret \"\"",
            "Elsewhere",
            "Ended",
            "Skipped",
            "leased, the keyholder's terminal: true",
            "UnlockAll",
        ]
    );
    assert_eq!(
        person.seen(),
        [
            "notify Working Unlocking /dev/sda2",
            "ask missing",
            "secret root Passphrase elsewhere Some(true)",
            "secret root Passphrase elsewhere None",
            "secret root Passphrase elsewhere None",
            "secret root Passphrase elsewhere None",
            "wait YubiKey present false",
            "lend",
            "take back",
        ]
    );
}

#[test]
fn a_keyholder_that_ends_too_soon_fails_the_run_and_none_that_starts_is_said() {
    let (client, keyholder) = Connection::pair().unwrap();
    drop(keyholder);

    let events = unlock(&Given::new(client), &Person::default(), &unlock_all()).unwrap();

    let Some(Event::Finished(Outcome::Failed { what, why, .. })) = events.last() else {
        panic!("{events:?}");
    };
    assert_eq!(what, "unable to unlock");
    assert!(
        why[0].starts_with("the keyholder did not finish: "),
        "{why:?}"
    );
    assert!(unlock(&Given::none(), &Person::default(), &unlock_all()).is_err());
}

#[test]
fn a_keyholder_asked_nothing_it_does_says_so_and_a_client_gone_gets_no_secret() {
    let (client, keyholder) = Connection::pair().unwrap();
    client.send(br#"{"version":1,"message":"ended"}"#).unwrap();

    let mut ran = false;
    serve(&keyholder, &FakeRunner::new(), |_, _| {
        ran = true;
        Vec::new()
    })
    .unwrap();

    assert!(!ran);
    let reply = FromKeyholder::decode(&client.receive(None).unwrap().unwrap()).unwrap();
    let FromKeyholder::Finished(events) = reply else {
        panic!("{reply:?}");
    };
    assert!(matches!(
        &events[0],
        Event::Finished(Outcome::Failed { why, .. }) if why[0].contains("asked nothing")
    ));

    // The client goes once it asked: every request then gets the answer
    // of no one at the console, and the events reach no one.
    let (client, keyholder) = Connection::pair().unwrap();
    client
        .send(&ToKeyholder::Unlock(unlock_all()).encode().unwrap())
        .unwrap();
    drop(client);
    let console = FakeRunner::new();
    let mut got = Vec::new();

    let result = serve(&keyholder, &console, |interaction, _| {
        got = ask_everything(interaction, &console);
        Vec::new()
    });

    assert_eq!(got[0], "Escape");
    assert!(got[1..5].iter().all(|answer| answer == "Ended"), "{got:?}");
    assert_eq!(
        got[5..],
        ["TimedOut", "leased, the keyholder's terminal: false"]
    );
    assert!(result.is_err());
}

/// The messages a peer sends as raw text, one a packet.
fn say(connection: &Connection, messages: &[&str]) {
    for message in messages {
        connection.send(message.as_bytes()).unwrap();
    }
}

#[test]
fn a_keyholder_out_of_turn_fails_the_run() {
    for script in [
        &[r#"{"version":1,"message":"returned"}"#][..],
        &[
            r#"{"version":1,"message":"lease"}"#,
            r#"{"version":1,"message":"answered","answer":true}"#,
        ],
    ] {
        let (client, keyholder) = Connection::pair().unwrap();
        say(&keyholder, script);

        let events = unlock(&Given::new(client), &Person::default(), &unlock_all()).unwrap();

        assert!(
            matches!(
                events.last(),
                Some(Event::Finished(Outcome::Failed { why, .. }))
                    if why[0].ends_with("a message out of turn")
            ),
            "{events:?}"
        );
    }
}

#[test]
fn a_client_out_of_turn_gets_no_secret_nor_answer() {
    let (client, keyholder) = Connection::pair().unwrap();
    client
        .send(&ToKeyholder::Unlock(unlock_all()).encode().unwrap())
        .unwrap();
    // Answers of the wrong kind; a secret shorter than it said; a query
    // the client does not wait for.
    say(
        &client,
        &[
            r#"{"version":1,"message":"ended"}"#,
            r#"{"version":1,"message":"secret","length":5}"#,
            "abc",
            r#"{"version":1,"message":"elsewhere"}"#,
            r#"{"version":1,"message":"query","query":"present"}"#,
        ],
    );
    drop(client);
    let mut got = Vec::new();

    let _ = serve(&keyholder, &FakeRunner::new(), |interaction, _| {
        got = ask_everything(interaction, &FakeRunner::new());
        Vec::new()
    });

    assert_eq!(got[..2], ["Escape", "Ended"]);
    assert_eq!(got[5], "TimedOut");
}
