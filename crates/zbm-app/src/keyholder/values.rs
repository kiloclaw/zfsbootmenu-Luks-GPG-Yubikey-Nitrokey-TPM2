//! The values inside the keyholder's messages, as JSON: what the person is
//! asked, told and waits for, and the events a run ends with; and the
//! fields of a message's object.

use serde_json::{Map, Value, json};
use zbm_platform::interaction::{Awaited, PromptRequest, SecretKind, Subject, Told};
use zbm_types::RetryDelay;

use super::messages::Unread;
use crate::model::{Event, Outcome, UnlockOutcome};
use crate::wire::text;

/// The boolean `key` of `object`.
pub(super) fn flag(object: &Map<String, Value>, key: &str) -> Option<bool> {
    object.get(key)?.as_bool()
}

/// The number `key` of `object`.
pub(super) fn number(object: &Map<String, Value>, key: &str) -> Option<u64> {
    object.get(key)?.as_u64()
}

/// The text `key` of `object`, owned.
pub(super) fn owned(object: &Map<String, Value>, key: &str) -> Option<String> {
    text(object, key).map(str::to_owned)
}

/// The text `key` of `object`, which may be `null`: `None` then.
///
/// # Errors
///
/// When it is neither, or missing.
pub(super) fn optional(object: &Map<String, Value>, key: &str) -> Result<Option<String>, Unread> {
    match object.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(Unread::Malformed),
    }
}

/// The texts of the list `key` of `object`.
pub(super) fn texts(object: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    object
        .get(key)?
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_owned))
        .collect()
}

/// The object inside `object` at `key`.
pub(super) fn inner<'o>(
    object: &'o Map<String, Value>,
    key: &str,
) -> Option<&'o Map<String, Value>> {
    object.get(key)?.as_object()
}

/// `kind` as JSON.
pub(super) fn secret_kind(kind: &SecretKind) -> Value {
    match kind {
        SecretKind::Passphrase => json!("passphrase"),
        SecretKind::Tpm2Pin => json!("tpm2 pin"),
        SecretKind::Fido2Pin => json!("fido2 pin"),
        SecretKind::YubiKeyPassphrase(slot) => json!({"yubikey passphrase": slot}),
        SecretKind::KeyFilePassphrase(file) => json!({"key file passphrase": file}),
        SecretKind::SecretKeyPassphrase(key) => json!({"secret key passphrase": key}),
        SecretKind::CardPin { serial, left } => {
            json!({"card pin": {"serial": serial, "left": left}})
        }
    }
}

/// The secret kind in `value`.
pub(super) fn read_secret_kind(value: &Value) -> Option<SecretKind> {
    if let Some(name) = value.as_str() {
        return match name {
            "passphrase" => Some(SecretKind::Passphrase),
            "tpm2 pin" => Some(SecretKind::Tpm2Pin),
            "fido2 pin" => Some(SecretKind::Fido2Pin),
            _ => None,
        };
    }
    let object = value.as_object()?;
    let (key, value) = object.iter().next().filter(|_| object.len() == 1)?;
    let inner = || value.as_str().map(str::to_owned);
    match key.as_str() {
        "yubikey passphrase" => inner().map(SecretKind::YubiKeyPassphrase),
        "key file passphrase" => inner().map(SecretKind::KeyFilePassphrase),
        "secret key passphrase" => inner().map(SecretKind::SecretKeyPassphrase),
        "card pin" => {
            let card = value.as_object()?;
            Some(SecretKind::CardPin {
                serial: owned(card, "serial")?,
                left: owned(card, "left")?,
            })
        }
        _ => None,
    }
}

/// `awaited` as JSON.
pub(super) fn awaited(awaited: &Awaited) -> Value {
    match awaited {
        Awaited::SecurityKey => json!("security key"),
        Awaited::OpenPgpKey => json!("openpgp key"),
        Awaited::YubiKey => json!("yubikey"),
        Awaited::KeyDevice { spec, file } => json!({"key device": {"spec": spec, "file": file}}),
    }
}

/// What is awaited in `value`.
pub(super) fn read_awaited(value: &Value) -> Option<Awaited> {
    match value.as_str() {
        Some("security key") => Some(Awaited::SecurityKey),
        Some("openpgp key") => Some(Awaited::OpenPgpKey),
        Some("yubikey") => Some(Awaited::YubiKey),
        Some(_) => None,
        None => {
            let device = value.as_object()?.get("key device")?.as_object()?;
            Some(Awaited::KeyDevice {
                spec: owned(device, "spec")?,
                file: owned(device, "file")?,
            })
        }
    }
}

/// The name of `told`.
pub(super) fn told_name(told: Told) -> &'static str {
    match told {
        Told::Working => "working",
        Told::Touch => "touch",
        Told::Refused => "refused",
        Told::Heading => "heading",
    }
}

/// `request` as JSON.
pub(super) fn prompt(request: &PromptRequest) -> Value {
    let subject = match &request.subject {
        Subject::Retry { name, device } => json!({"retry": {"name": name, "device": device}}),
        Subject::Missing(spec) => json!({"missing": spec}),
    };
    json!({
        "subject": subject,
        "delay": request.delay.map(RetryDelay::seconds),
        "text": request.message,
        "on_return": request.on_return,
        "on_escape": request.on_escape,
    })
}

/// The prompt in `object`.
pub(super) fn read_prompt(object: &Map<String, Value>) -> Option<PromptRequest> {
    let subject = inner(object, "subject")?;
    let subject = if let Some(retry) = inner(subject, "retry") {
        Subject::Retry {
            name: owned(retry, "name")?,
            device: owned(retry, "device")?,
        }
    } else {
        Subject::Missing(owned(subject, "missing")?)
    };
    let delay = match object.get("delay")? {
        Value::Null => None,
        seconds => Some(RetryDelay::new(seconds.as_u64()?)?),
    };
    Some(PromptRequest {
        subject,
        delay,
        message: owned(object, "text")?,
        on_return: owned(object, "on_return")?,
        on_escape: owned(object, "on_escape")?,
    })
}

/// `outcome` as JSON.
pub(super) fn outcome(outcome: &Outcome) -> Value {
    match outcome {
        Outcome::Done(text) => json!({"done": text}),
        Outcome::Warning(text) => json!({"warning": text}),
        Outcome::Failed { what, why, next } => {
            json!({"failed": {"what": what, "why": why, "next": next}})
        }
        Outcome::Cancelled => json!("cancelled"),
    }
}

/// The outcome in `value`.
pub(super) fn read_outcome(value: &Value) -> Option<Outcome> {
    if value.as_str() == Some("cancelled") {
        return Some(Outcome::Cancelled);
    }
    let object = value.as_object()?;
    if let Some(text) = text(object, "done") {
        return Some(Outcome::Done(text.to_owned()));
    }
    if let Some(text) = text(object, "warning") {
        return Some(Outcome::Warning(text.to_owned()));
    }
    let failed = inner(object, "failed")?;
    Some(Outcome::Failed {
        what: owned(failed, "what")?,
        why: texts(failed, "why")?,
        next: texts(failed, "next")?,
    })
}

/// `event`, one of those the volumes' actions end with, as JSON.
pub(super) fn event(event: &Event) -> Option<Value> {
    Some(match event {
        Event::UnlockFinished { volume, outcome } => {
            let outcome = match outcome {
                UnlockOutcome::Opened => json!("opened"),
                UnlockOutcome::Skipped => json!("skipped"),
                UnlockOutcome::Failed(why) => json!({"failed": why}),
            };
            json!({"unlock finished": {"volume": volume, "outcome": outcome}})
        }
        Event::Finished(ended) => json!({"finished": outcome(ended)}),
        _ => return None,
    })
}

/// The event in `value`.
pub(super) fn read_event(value: &Value) -> Option<Event> {
    let object = value.as_object()?;
    if let Some(ended) = object.get("finished") {
        return read_outcome(ended).map(Event::Finished);
    }
    let finished = inner(object, "unlock finished")?;
    let outcome = match finished.get("outcome")? {
        Value::String(name) if name == "opened" => UnlockOutcome::Opened,
        Value::String(name) if name == "skipped" => UnlockOutcome::Skipped,
        Value::Object(failed) => UnlockOutcome::Failed(owned(failed, "failed")?),
        _ => return None,
    };
    Some(Event::UnlockFinished {
        volume: owned(finished, "volume")?,
        outcome,
    })
}
