//! The messages between a client and the keyholder, and their JSON. A
//! secret is never part of one: [`ToKeyholder::Secret`] says how long it
//! is, and its bytes follow as a packet of their own, sent from and
//! received into memory that is wiped.

use std::path::PathBuf;

use serde_json::{Map, Value, json};
use zbm_crypt::OpenOptions;
use zbm_platform::interaction::{Answer, Awaited, PromptRequest, SecretKind, Told, Wait};

use super::VERSION;
use super::values::{
    awaited, event, flag, inner, number, optional, owned, prompt, read_awaited, read_event,
    read_prompt, read_secret_kind, secret_kind, told_name,
};
use crate::model::{Action, Event};
use crate::wire::{message, text};

/// What a client asks the keyholder, first and once: the action, one of
/// those that open volumes, and whether volumes that appeared late are
/// retried (`LUKS_RETRY`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnlockRequest {
    /// [`Action::UnlockAll`], [`Action::Open`] or
    /// [`Action::OpenWithKeyFile`].
    pub action: Action,
    /// Whether this run retries the volumes that appeared late.
    pub retry: bool,
}

/// What the keyholder asks for a secret, as
/// `zbm_platform::interaction::SecretRequest` without its borrows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretAsk {
    /// The name of the volume's mapping.
    pub name: String,
    /// Its LUKS device.
    pub device: String,
    /// What is asked.
    pub kind: SecretKind,
    /// The attempt, from 1.
    pub attempt: u32,
    /// The attempts allowed, when limited.
    pub tries: Option<u32>,
    /// The volume described for the person.
    pub description: Option<String>,
    /// Whether another program may open the volume meanwhile, which the
    /// client then asks with [`Query::OpenedElsewhere`].
    pub may_open_elsewhere: bool,
}

/// What the keyholder waits for, as
/// `zbm_platform::interaction::WaitRequest` without its borrows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitAsk {
    /// The name of the volume's mapping.
    pub name: String,
    /// Its LUKS device.
    pub device: String,
    /// What it waits for, which the client asks about with
    /// [`Query::Present`].
    pub awaited: Awaited,
    /// How long, in seconds.
    pub seconds: u64,
}

/// What the keyholder says along the way, as
/// `zbm_platform::interaction::Tell` without its borrows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notice {
    /// What kind of message.
    pub told: Told,
    /// The message.
    pub text: String,
    /// The part to highlight.
    pub highlight: Option<String>,
}

/// What a client asks the keyholder while it answers a request: what only
/// the keyholder can see.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Query {
    /// Whether another program opened the volume of the secret asked.
    OpenedElsewhere,
    /// Whether what the keyholder waits for is there.
    Present,
}

/// A message from the keyholder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FromKeyholder {
    /// A timed prompt: [`ToKeyholder::Answer`] answers it.
    Ask(PromptRequest),
    /// A secret: [`ToKeyholder::Secret`], [`ToKeyholder::Elsewhere`] or
    /// [`ToKeyholder::Ended`] answers it.
    Secret(SecretAsk),
    /// A wait for a device: [`ToKeyholder::Waited`] answers it.
    Wait(WaitAsk),
    /// Something to say; nothing answers it.
    Notify(Notice),
    /// A program of the keyholder needs the console:
    /// [`ToKeyholder::Leased`] lends it.
    Lease,
    /// The program that had the console ended.
    Returned,
    /// The answer to a [`Query`].
    Answered(bool),
    /// The run ended, with its events; the keyholder ends too.
    Finished(Vec<Event>),
}

/// A message to the keyholder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToKeyholder {
    /// The run to do.
    Unlock(UnlockRequest),
    /// The answer to [`FromKeyholder::Ask`].
    Answer(Answer),
    /// The secret asked, `length` bytes long, which follow in a packet of
    /// their own unless there are none.
    Secret {
        /// Its length.
        length: usize,
    },
    /// Nothing: another program opened the volume meanwhile.
    Elsewhere,
    /// Nothing: the console has no more input.
    Ended,
    /// How waiting ended.
    Waited(Wait),
    /// The console is lent: with `console`, the keyholder's program uses
    /// the keyholder's own terminal, which the client stepped aside from.
    Leased {
        /// Whether the client lent its terminal, rather than leaving the
        /// program to the keyholder's quiet runner.
        console: bool,
    },
    /// A question about what the keyholder sees.
    Query(Query),
}

/// Why a message was not read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unread {
    /// It is of another version of the protocol.
    Version(u64),
    /// It is none of the protocol.
    Malformed,
}

/// The message of `kind`, with `fields`, as bytes.
fn encoded(kind: &str, fields: Value) -> Vec<u8> {
    let mut object = Map::new();
    object.insert("version".to_owned(), json!(VERSION));
    object.insert("message".to_owned(), json!(kind));
    if let Value::Object(fields) = fields {
        object.extend(fields);
    }
    Value::Object(object).to_string().into_bytes()
}

/// The kind and object of a message of this version.
fn decoded(bytes: &[u8]) -> Result<(String, Map<String, Value>), Unread> {
    let (version, object) = message(bytes).ok_or(Unread::Malformed)?;
    if version != VERSION {
        return Err(Unread::Version(version));
    }
    let kind = text(&object, "message")
        .ok_or(Unread::Malformed)?
        .to_owned();
    Ok((kind, object))
}

impl UnlockRequest {
    /// Its fields.
    fn fields(&self) -> Option<Value> {
        let action = match &self.action {
            Action::UnlockAll => json!({"action": "unlock all"}),
            Action::Open { device, name } => json!({
                "action": "open",
                "device": device,
                "name": name.as_ref().map(ToString::to_string),
            }),
            Action::OpenWithKeyFile {
                device,
                name,
                key_file,
                options,
            } => json!({
                "action": "open with key file",
                "device": device.as_str(),
                "name": name.as_str(),
                "key_file": key_file.to_string_lossy(),
                "allow_discards": options.allow_discards,
                "read_only": options.read_only,
            }),
            _ => return None,
        };
        Some(json!({"unlock": action, "retry": self.retry}))
    }

    /// The request in `object`.
    fn read(object: &Map<String, Value>) -> Option<UnlockRequest> {
        let unlock = inner(object, "unlock")?;
        let action = match text(unlock, "action")? {
            "unlock all" => Action::UnlockAll,
            "open" => Action::Open {
                device: owned(unlock, "device")?,
                name: optional(unlock, "name")
                    .ok()?
                    .map(|name| name.parse())
                    .transpose()
                    .ok()?,
            },
            "open with key file" => Action::OpenWithKeyFile {
                device: text(unlock, "device")?.parse().ok()?,
                name: text(unlock, "name")?.parse().ok()?,
                key_file: PathBuf::from(text(unlock, "key_file")?),
                options: OpenOptions {
                    allow_discards: flag(unlock, "allow_discards")?,
                    read_only: flag(unlock, "read_only")?,
                },
            },
            _ => return None,
        };
        Some(UnlockRequest {
            action,
            retry: flag(object, "retry")?,
        })
    }
}

/// The name of `answer`.
fn answer_name(answer: Answer) -> &'static str {
    match answer {
        Answer::Continue => "continue",
        Answer::Escape => "escape",
    }
}

/// The name of `wait`.
fn wait_name(wait: Wait) -> &'static str {
    match wait {
        Wait::Present => "present",
        Wait::Skipped => "skipped",
        Wait::TimedOut => "timed out",
    }
}

/// The name of `query`.
fn query_name(query: Query) -> &'static str {
    match query {
        Query::OpenedElsewhere => "opened elsewhere",
        Query::Present => "present",
    }
}

impl ToKeyholder {
    /// The message, as bytes; `None` for an [`ToKeyholder::Unlock`] of an
    /// action the keyholder does not perform.
    pub fn encode(&self) -> Option<Vec<u8>> {
        Some(match self {
            ToKeyholder::Unlock(request) => encoded("unlock", request.fields()?),
            ToKeyholder::Answer(answer) => {
                encoded("answer", json!({"answer": answer_name(*answer)}))
            }
            ToKeyholder::Secret { length } => encoded("secret", json!({"length": length})),
            ToKeyholder::Elsewhere => encoded("elsewhere", Value::Null),
            ToKeyholder::Ended => encoded("ended", Value::Null),
            ToKeyholder::Waited(wait) => encoded("waited", json!({"wait": wait_name(*wait)})),
            ToKeyholder::Leased { console } => encoded("leased", json!({"console": console})),
            ToKeyholder::Query(query) => encoded("query", json!({"query": query_name(*query)})),
        })
    }

    /// The message a client sent as `bytes`.
    ///
    /// # Errors
    ///
    /// When it is of another version, or none of the protocol.
    pub fn decode(bytes: &[u8]) -> Result<ToKeyholder, Unread> {
        let (kind, object) = decoded(bytes)?;
        let message = match kind.as_str() {
            "unlock" => UnlockRequest::read(&object).map(ToKeyholder::Unlock),
            "answer" => match text(&object, "answer") {
                Some("continue") => Some(ToKeyholder::Answer(Answer::Continue)),
                Some("escape") => Some(ToKeyholder::Answer(Answer::Escape)),
                _ => None,
            },
            "secret" => number(&object, "length")
                .and_then(|length| usize::try_from(length).ok())
                .filter(|length| *length <= zbm_sys::socket::MAX_MESSAGE)
                .map(|length| ToKeyholder::Secret { length }),
            "elsewhere" => Some(ToKeyholder::Elsewhere),
            "ended" => Some(ToKeyholder::Ended),
            "waited" => [Wait::Present, Wait::Skipped, Wait::TimedOut]
                .into_iter()
                .find(|wait| text(&object, "wait") == Some(wait_name(*wait)))
                .map(ToKeyholder::Waited),
            "leased" => flag(&object, "console").map(|console| ToKeyholder::Leased { console }),
            "query" => [Query::OpenedElsewhere, Query::Present]
                .into_iter()
                .find(|query| text(&object, "query") == Some(query_name(*query)))
                .map(ToKeyholder::Query),
            _ => None,
        };
        message.ok_or(Unread::Malformed)
    }
}

impl FromKeyholder {
    /// The message, as bytes. Events other than those the volumes' actions
    /// end with are left out.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            FromKeyholder::Ask(request) => encoded("ask", prompt(request)),
            FromKeyholder::Secret(ask) => encoded(
                "secret",
                json!({
                    "name": ask.name,
                    "device": ask.device,
                    "kind": secret_kind(&ask.kind),
                    "attempt": ask.attempt,
                    "tries": ask.tries,
                    "description": ask.description,
                    "may_open_elsewhere": ask.may_open_elsewhere,
                }),
            ),
            FromKeyholder::Wait(ask) => encoded(
                "wait",
                json!({
                    "name": ask.name,
                    "device": ask.device,
                    "awaited": awaited(&ask.awaited),
                    "seconds": ask.seconds,
                }),
            ),
            FromKeyholder::Notify(notice) => encoded(
                "notify",
                json!({
                    "told": told_name(notice.told),
                    "text": notice.text,
                    "highlight": notice.highlight,
                }),
            ),
            FromKeyholder::Lease => encoded("lease", Value::Null),
            FromKeyholder::Returned => encoded("returned", Value::Null),
            FromKeyholder::Answered(answer) => encoded("answered", json!({"answer": answer})),
            FromKeyholder::Finished(events) => {
                let events: Vec<Value> = events.iter().filter_map(event).collect();
                encoded("finished", json!({"events": events}))
            }
        }
    }

    /// The message the keyholder sent as `bytes`.
    ///
    /// # Errors
    ///
    /// When it is of another version, or none of the protocol.
    pub fn decode(bytes: &[u8]) -> Result<FromKeyholder, Unread> {
        let (kind, object) = decoded(bytes)?;
        let message = match kind.as_str() {
            "ask" => read_prompt(&object).map(FromKeyholder::Ask),
            "secret" => read_secret_ask(&object).map(FromKeyholder::Secret),
            "wait" => read_wait_ask(&object).map(FromKeyholder::Wait),
            "notify" => read_notice(&object).map(FromKeyholder::Notify),
            "lease" => Some(FromKeyholder::Lease),
            "returned" => Some(FromKeyholder::Returned),
            "answered" => flag(&object, "answer").map(FromKeyholder::Answered),
            "finished" => object
                .get("events")
                .and_then(Value::as_array)
                .and_then(|events| events.iter().map(read_event).collect())
                .map(FromKeyholder::Finished),
            _ => None,
        };
        message.ok_or(Unread::Malformed)
    }
}

/// The request for a secret in `object`.
fn read_secret_ask(object: &Map<String, Value>) -> Option<SecretAsk> {
    let tries = match object.get("tries")? {
        Value::Null => None,
        tries => Some(u32::try_from(tries.as_u64()?).ok()?),
    };
    Some(SecretAsk {
        name: owned(object, "name")?,
        device: owned(object, "device")?,
        kind: read_secret_kind(object.get("kind")?)?,
        attempt: u32::try_from(number(object, "attempt")?).ok()?,
        tries,
        description: optional(object, "description").ok()?,
        may_open_elsewhere: flag(object, "may_open_elsewhere")?,
    })
}

/// The wait in `object`.
fn read_wait_ask(object: &Map<String, Value>) -> Option<WaitAsk> {
    Some(WaitAsk {
        name: owned(object, "name")?,
        device: owned(object, "device")?,
        awaited: read_awaited(object.get("awaited")?)?,
        seconds: number(object, "seconds")?,
    })
}

/// The notice in `object`.
fn read_notice(object: &Map<String, Value>) -> Option<Notice> {
    let told = [Told::Working, Told::Touch, Told::Refused, Told::Heading]
        .into_iter()
        .find(|told| text(object, "told") == Some(told_name(*told)))?;
    Some(Notice {
        told,
        text: owned(object, "text")?,
        highlight: optional(object, "highlight").ok()?,
    })
}
