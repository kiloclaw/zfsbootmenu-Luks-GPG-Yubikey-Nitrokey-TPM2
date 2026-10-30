//! The keyholder's end: the run its client asks for, with an
//! [`Interaction`] that asks the client.

use std::io;

use zbm_platform::Runner;
use zbm_platform::Secret;
use zbm_platform::interaction::{
    Answer, Asked, Interaction, PromptRequest, SecretRequest, Tell, Wait, WaitRequest,
};
use zbm_sys::socket::Connection;

use super::messages::{
    FromKeyholder, Notice, Query, SecretAsk, ToKeyholder, UnlockRequest, WaitAsk,
};
use crate::model::{Event, Outcome};

/// Answers the one request of the client at the other end of `connection`:
/// `run` does it, with an [`Interaction`] that asks the client, and
/// `console` the runner whose programs reach the terminal the client lends.
/// The events `run` returns go back to the client.
///
/// # Errors
///
/// When the client is gone before the events reach it.
pub fn serve(
    connection: &Connection,
    console: &dyn Runner,
    run: impl FnOnce(&dyn Interaction, &UnlockRequest) -> Vec<Event>,
) -> io::Result<()> {
    let events = match receive(connection) {
        Some(ToKeyholder::Unlock(request)) => {
            tracing::info!(action = ?request.action, "the keyholder performs an action");
            let client = Client {
                connection,
                console,
            };
            run(&client, &request)
        }
        _ => vec![Event::Finished(Outcome::Failed {
            what: "unable to unlock".to_owned(),
            why: vec!["the keyholder was asked nothing it does".to_owned()],
            next: Vec::new(),
        })],
    };
    connection.send(&FromKeyholder::Finished(events).encode())
}

/// The client's next message; `None` when it is gone or sent none of the
/// protocol.
fn receive(connection: &Connection) -> Option<ToKeyholder> {
    let bytes = connection.receive(None).ok()??;
    ToKeyholder::decode(&bytes).ok()
}

/// The client, as the person at the console.
struct Client<'c> {
    connection: &'c Connection,
    console: &'c dyn Runner,
}

impl Client<'_> {
    /// Sends `message`; whether it went.
    fn is_sent(&self, message: &FromKeyholder) -> bool {
        self.connection.send(&message.encode()).is_ok()
    }

    /// The client's reply to what was sent, answering its queries with
    /// `answer` meanwhile; `None` when it is gone.
    fn reply(&self, answer: &dyn Fn(Query) -> bool) -> Option<ToKeyholder> {
        loop {
            match receive(self.connection)? {
                ToKeyholder::Query(query) => {
                    if !self.is_sent(&FromKeyholder::Answered(answer(query))) {
                        return None;
                    }
                }
                reply => return Some(reply),
            }
        }
    }

    /// The `length` bytes of a secret, which follow in a packet of their
    /// own; `None` when they do not.
    fn secret_bytes(&self, length: usize) -> Option<Secret> {
        if length == 0 {
            return Some(Secret::new());
        }
        let mut bytes = self.connection.receive(None).ok()??;
        if bytes.len() != length {
            zbm_sys::wipe_vec(&mut bytes);
            return None;
        }
        Some(Secret::from_vec(bytes))
    }
}

impl Interaction for Client<'_> {
    fn ask(&self, request: &PromptRequest) -> Answer {
        if !self.is_sent(&FromKeyholder::Ask(request.clone())) {
            return Answer::Escape;
        }
        match self.reply(&|_| false) {
            Some(ToKeyholder::Answer(answer)) => answer,
            _ => Answer::Escape,
        }
    }

    fn secret(&self, request: &SecretRequest<'_>) -> Asked {
        let ask = SecretAsk {
            name: request.volume.name.to_owned(),
            device: request.volume.device.to_owned(),
            kind: request.kind.clone(),
            attempt: request.attempt,
            tries: request.tries,
            description: request.description.map(str::to_owned),
            may_open_elsewhere: request.opened_elsewhere.is_some(),
        };
        if !self.is_sent(&FromKeyholder::Secret(ask)) {
            return Asked::Ended;
        }
        let opened_elsewhere = |query| {
            query == Query::OpenedElsewhere
                && request.opened_elsewhere.is_some_and(|opened| opened())
        };
        match self.reply(&opened_elsewhere) {
            Some(ToKeyholder::Secret { length }) => self
                .secret_bytes(length)
                .map_or(Asked::Ended, Asked::Secret),
            Some(ToKeyholder::Elsewhere) => Asked::Elsewhere,
            _ => Asked::Ended,
        }
    }

    fn wait(&self, request: &WaitRequest<'_>) -> Wait {
        let ask = WaitAsk {
            name: request.volume.name.to_owned(),
            device: request.volume.device.to_owned(),
            awaited: request.awaited.clone(),
            seconds: request.seconds,
        };
        if !self.is_sent(&FromKeyholder::Wait(ask)) {
            return Wait::TimedOut;
        }
        match self.reply(&|query| query == Query::Present && (request.present)()) {
            Some(ToKeyholder::Waited(wait)) => wait,
            _ => Wait::TimedOut,
        }
    }

    fn notify(&self, tell: Tell<'_>) {
        let _ = self.is_sent(&FromKeyholder::Notify(Notice {
            told: tell.told,
            text: tell.text.to_owned(),
            highlight: tell.highlight.map(str::to_owned),
        }));
    }

    /// The client's front end steps aside, and the program gets this
    /// keyholder's terminal, the client's own, when it lent it.
    fn lease_console(&self, run: &mut dyn FnMut(Option<&dyn Runner>)) {
        let lent = self.is_sent(&FromKeyholder::Lease)
            && matches!(
                self.reply(&|_| false),
                Some(ToKeyholder::Leased { console: true })
            );
        run(lent.then_some(self.console));
        let _ = self.is_sent(&FromKeyholder::Returned);
    }
}
