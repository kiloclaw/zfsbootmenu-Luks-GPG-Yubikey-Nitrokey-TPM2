//! The keyholder (ROADMAP.md, SB10; ADR 0002, zone Z1): the process that
//! opens LUKS volumes, and so the only one that holds what opens them, the
//! secrets cached for several volumes, decrypted key files and unsealed
//! keys. A client, start-up or `zluks`, starts one for each unlocking run
//! ([`Keyholders`]), sends it the action ([`UnlockRequest`]), and answers
//! what it asks the person at the console with the client's own
//! [`Interaction`], until the keyholder says how the run ended and ends.
//! The keyholder runs the same code as before, [`crate::Volumes`], with an
//! [`Interaction`] that asks its client ([`serve`]); so the zone moved,
//! not the unlocking.
//!
//! A typed secret goes from the client's front end to the keyholder and is
//! wiped on the way: it never rests in the client. Its bytes are a packet
//! of their own, which [`ToKeyholder::Secret`] announces, sent from and
//! received into memory that is wiped.
//!
//! The two ends are a socket pair (`SOCK_SEQPACKET`): one message a packet,
//! each a JSON object with the [`VERSION`] of the protocol, its kind in
//! `message`. The client sends the request; the keyholder asks, the client
//! answers, until the keyholder has finished:
//!
//! ```text
//! {"version":1,"message":"unlock","unlock":{"action":"unlock all"},
//!  "retry":false}
//!     {"version":1,"message":"notify","told":"working","text":"…",
//!      "highlight":null}
//!     {"version":1,"message":"secret","name":"root","device":"/dev/sda2",
//!      "kind":"passphrase","attempt":1,"tries":3,"description":"…",
//!      "may_open_elsewhere":true}
//! {"version":1,"message":"query","query":"opened elsewhere"}
//!     {"version":1,"message":"answered","answer":false}
//! {"version":1,"message":"secret","length":9}
//! (9 bytes)
//!     {"version":1,"message":"finished","events":[…]}
//! ```
//!
//! While it answers a secret or a wait, the client may ask what only the
//! keyholder sees ([`Query`]): whether another program opened the volume,
//! whether the device waited for is there. A key script or `fido2-assert`
//! that needs the console borrows it ([`FromKeyholder::Lease`]): the
//! client's front end steps aside until the keyholder says the program
//! [`FromKeyholder::Returned`].

mod messages;
mod serve;
mod values;

use std::io;

use zbm_platform::interaction::{Interaction, SecretRequest, Tell, VolumeRef, WaitRequest};
use zbm_sys::socket::Connection;

pub use self::messages::{
    FromKeyholder, Notice, Query, SecretAsk, ToKeyholder, UnlockRequest, Unread, WaitAsk,
};
pub use self::serve::serve;
use crate::model::{Action, Event, Outcome};

/// The version of the protocol.
pub const VERSION: u64 = 1;

/// The environment variable that holds the number of the keyholder's end of
/// the socket pair, in the keyholder.
pub const KEYHOLDER_FD: &str = "ZBM_KEYHOLDER_FD";

/// A keyholder that runs: the client's end of its connection. Dropping it
/// ends the keyholder, if it did not end yet, and collects it.
pub trait Keyholder {
    /// The client's end of the connection.
    fn connection(&self) -> &Connection;
}

/// Starts keyholders, one for each unlocking run.
pub trait Keyholders: Sync {
    /// A new keyholder, connected.
    ///
    /// # Errors
    ///
    /// When it cannot be started.
    fn start(&self) -> io::Result<Box<dyn Keyholder + '_>>;
}

/// Whether `action` opens volumes, which the keyholder does, as it holds
/// their secrets. Closing one takes none.
pub fn is_unlocking(action: &Action) -> bool {
    matches!(
        action,
        Action::UnlockAll | Action::Open { .. } | Action::OpenWithKeyFile { .. }
    )
}

/// Runs `request` in a keyholder of `keyholders`, answering what it asks
/// with `interaction`; the run's events, the last of them how it ended.
///
/// # Errors
///
/// When no keyholder can be started: nothing was done.
pub fn unlock(
    keyholders: &dyn Keyholders,
    interaction: &dyn Interaction,
    request: &UnlockRequest,
) -> io::Result<Vec<Event>> {
    let keyholder = keyholders.start()?;
    let connection = keyholder.connection();
    let ended = request
        .encode()
        .ok_or_else(|| "the keyholder does not perform that action".to_owned())
        .and_then(|message| send(connection, &message))
        .and_then(|()| answer(connection, interaction));
    Ok(ended.unwrap_or_else(|why| {
        tracing::error!(%why, "the keyholder did not finish");
        vec![Event::Finished(Outcome::Failed {
            what: "unable to unlock".to_owned(),
            why: vec![format!("the keyholder did not finish: {why}")],
            next: Vec::new(),
        })]
    }))
}

impl UnlockRequest {
    /// The message that carries it; `None` for an action the keyholder
    /// does not perform.
    fn encode(&self) -> Option<Vec<u8>> {
        ToKeyholder::Unlock(self.clone()).encode()
    }
}

/// Sends `message`, or says why not.
fn send(connection: &Connection, message: &[u8]) -> Result<(), String> {
    connection.send(message).map_err(|error| error.to_string())
}

/// Sends `message` to the keyholder.
fn tell(connection: &Connection, message: &ToKeyholder) -> Result<(), String> {
    let bytes = message
        .encode()
        .ok_or_else(|| "a message without an encoding".to_owned())?;
    send(connection, &bytes)
}

/// The keyholder's next message.
fn next(connection: &Connection) -> Result<FromKeyholder, String> {
    match connection.receive(None) {
        Ok(Some(bytes)) if !bytes.is_empty() => {
            FromKeyholder::decode(&bytes).map_err(|unread| format!("{unread:?} message"))
        }
        Ok(_) => Err("it ended".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// Answers the keyholder's requests with `interaction` until it finished;
/// the events it finished with.
fn answer(connection: &Connection, interaction: &dyn Interaction) -> Result<Vec<Event>, String> {
    loop {
        match next(connection)? {
            FromKeyholder::Ask(request) => {
                tell(connection, &ToKeyholder::Answer(interaction.ask(&request)))?;
            }
            FromKeyholder::Secret(ask) => give_secret(connection, interaction, &ask)?,
            FromKeyholder::Wait(ask) => {
                let present = || is_so(connection, Query::Present);
                let request = WaitRequest {
                    volume: VolumeRef {
                        name: &ask.name,
                        device: &ask.device,
                    },
                    awaited: ask.awaited.clone(),
                    seconds: ask.seconds,
                    present: &present,
                };
                tell(connection, &ToKeyholder::Waited(interaction.wait(&request)))?;
            }
            FromKeyholder::Notify(notice) => interaction.notify(Tell {
                told: notice.told,
                text: &notice.text,
                highlight: notice.highlight.as_deref(),
            }),
            FromKeyholder::Lease => {
                let mut lent = Ok(());
                interaction.lease_console(&mut |runner| {
                    lent = lend(connection, runner.is_some());
                });
                lent?;
            }
            FromKeyholder::Finished(events) => return Ok(events),
            FromKeyholder::Returned | FromKeyholder::Answered(_) => {
                return Err("a message out of turn".to_owned());
            }
        }
    }
}

/// Asks `interaction` for the secret `ask` describes, and gives it to the
/// keyholder: the secret's bytes go from the memory it was typed into,
/// which is wiped when it is dropped here.
fn give_secret(
    connection: &Connection,
    interaction: &dyn Interaction,
    ask: &SecretAsk,
) -> Result<(), String> {
    let elsewhere = || is_so(connection, Query::OpenedElsewhere);
    let request = SecretRequest {
        volume: VolumeRef {
            name: &ask.name,
            device: &ask.device,
        },
        kind: ask.kind.clone(),
        attempt: ask.attempt,
        tries: ask.tries,
        opened_elsewhere: ask
            .may_open_elsewhere
            .then_some(&elsewhere as &dyn Fn() -> bool),
        description: ask.description.as_deref(),
    };
    match interaction.secret(&request) {
        zbm_platform::interaction::Asked::Secret(secret) => {
            let length = secret.len();
            tell(connection, &ToKeyholder::Secret { length })?;
            if length > 0 {
                send(connection, secret.expose())?;
            }
            Ok(())
        }
        zbm_platform::interaction::Asked::Elsewhere => tell(connection, &ToKeyholder::Elsewhere),
        zbm_platform::interaction::Asked::Ended => tell(connection, &ToKeyholder::Ended),
    }
}

/// Whether the keyholder answers `query` yes; not when it does not answer.
fn is_so(connection: &Connection, query: Query) -> bool {
    tell(connection, &ToKeyholder::Query(query)).is_ok()
        && matches!(next(connection), Ok(FromKeyholder::Answered(true)))
}

/// Lends the console to the keyholder's program, `console` when the front
/// end stepped aside from its terminal, until the program returned it.
fn lend(connection: &Connection, console: bool) -> Result<(), String> {
    tell(connection, &ToKeyholder::Leased { console })?;
    match next(connection)? {
        FromKeyholder::Returned => Ok(()),
        _ => Err("a message out of turn".to_owned()),
    }
}

#[cfg(test)]
pub(crate) mod tests;
