//! The journal: every event as one line of JSON.
//!
//! A line looks like this (wrapped here):
//!
//! ```json
//! {"pid":412,"seq":3,"level":"INFO","target":"zbm_crypt","message":"opened",
//!  "fields":{"name":"cryptroot"},
//!  "spans":[{"span":"unlock","device":"/dev/sda2"},
//!           {"span":"key_method","method":"key file"}]}
//! ```
//!
//! `pid` and `seq` identify a line: several programs may append to one
//! journal, and each numbers its lines from 0. `spans` lists the spans the
//! event happened in, outermost first, each with its name and fields.
//!
//! Secrets are kept out twice. By type: `Secret` prints as `Secret(..)`
//! wherever it is recorded. By name: the value of a field named for a secret
//! ([`is_secret_field`]) is written as `"[redacted]"`, whatever its type, so a
//! passphrase recorded by mistake as text does not reach the journal either.

use std::fmt;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Map, Value};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// What a redacted field's value is replaced with.
pub const REDACTED: &str = "[redacted]";

/// Whether a field named `name` holds a secret, so that its value is never
/// written: `passphrase`, `password`, `pin`, `secret`, `challenge`,
/// `response`, or a name ending in `_` and one of them.
pub fn is_secret_field(name: &str) -> bool {
    const SECRET: [&str; 6] = [
        "passphrase",
        "password",
        "pin",
        "secret",
        "challenge",
        "response",
    ];
    SECRET.iter().any(|secret| {
        name == *secret
            || name
                .strip_suffix(secret)
                .is_some_and(|prefix| prefix.ends_with('_'))
    })
}

/// A [`tracing_subscriber::Layer`] that writes each event as a line of JSON.
///
/// Writing never fails the program: a journal that cannot be written loses
/// lines, nothing else.
pub struct JournalLayer<W> {
    /// Where the lines go.
    writer: Mutex<W>,
    /// The number of the next line.
    seq: AtomicU64,
    /// The bytes this layer may still write; `u64::MAX` for no limit.
    room: AtomicU64,
    /// A copy of every line, for tests.
    copy: Lines,
}

impl<W: Write> JournalLayer<W> {
    /// A journal that writes to `writer`.
    pub fn new(writer: W) -> JournalLayer<W> {
        JournalLayer {
            writer: Mutex::new(writer),
            seq: AtomicU64::new(0),
            room: AtomicU64::new(u64::MAX),
            copy: Lines::default(),
        }
    }

    /// The same journal, writing at most `bytes` bytes, so that a program
    /// that runs for long cannot fill the memory file system. The line that
    /// would go past the limit is replaced by one saying that the journal is
    /// full, and later lines are left out.
    #[must_use]
    pub fn with_limit(self, bytes: u64) -> JournalLayer<W> {
        self.room.store(bytes, Ordering::Relaxed);
        self
    }

    /// A handle on the lines written so far, parsed, for tests and for
    /// `zbm debug`.
    pub fn lines(&self) -> Lines {
        self.copy.clone()
    }

    /// Writes one line, unless the journal is full.
    fn write(&self, value: Value) {
        let mut text = value.to_string();
        text.push('\n');
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        let room = self.room.load(Ordering::Relaxed);
        if room == 0 {
            return;
        }
        let length = u64::try_from(text.len()).unwrap_or(u64::MAX);
        if room != u64::MAX {
            if length > room {
                self.room.store(0, Ordering::Relaxed);
                let full = serde_json::json!({
                    "pid": std::process::id(),
                    "seq": value.get("seq"),
                    "level": "WARN",
                    "target": module_path!(),
                    "message": FULL,
                    "fields": {},
                    "spans": [],
                });
                let full = format!("{full}\n");
                let _ = writer.write_all(full.as_bytes());
                let _ = writer.flush();
                return;
            }
            self.room.store(room - length, Ordering::Relaxed);
        }
        let _ = writer.write_all(text.as_bytes());
        let _ = writer.flush();
        drop(writer);
        self.copy.push(value);
    }
}

impl<S, W> tracing_subscriber::Layer<S> for JournalLayer<W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: Write + Send + 'static,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        if let Some(span) = context.span(id) {
            span.extensions_mut().insert(fields);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, context: Context<'_, S>) {
        if let Some(span) = context.span(id)
            && let Some(fields) = span.extensions_mut().get_mut::<Fields>()
        {
            values.record(fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let message = fields.0.remove("message").unwrap_or(Value::Null);
        let spans: Vec<Value> = context
            .event_scope(event)
            .map(|scope| {
                scope
                    .from_root()
                    .map(|span| {
                        let mut object = Map::new();
                        object.insert("span".to_owned(), span.name().into());
                        if let Some(fields) = span.extensions().get::<Fields>() {
                            object.extend(fields.0.clone());
                        }
                        Value::Object(object)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let metadata = event.metadata();
        let mut line = Map::new();
        line.insert("pid".to_owned(), std::process::id().into());
        line.insert(
            "seq".to_owned(),
            self.seq.fetch_add(1, Ordering::Relaxed).into(),
        );
        line.insert("level".to_owned(), metadata.level().as_str().into());
        line.insert("target".to_owned(), metadata.target().into());
        line.insert("message".to_owned(), message);
        line.insert("fields".to_owned(), Value::Object(fields.0));
        line.insert("spans".to_owned(), spans.into());
        self.write(Value::Object(line));
    }
}

/// The line written in place of the one that would fill the journal.
const FULL: &str = "the journal is full; later lines are left out";

/// The fields of a span or an event, as JSON values.
#[derive(Clone, Debug, Default)]
struct Fields(Map<String, Value>);

impl Fields {
    /// Records `value` for `field`, or [`REDACTED`] if the field is named for
    /// a secret.
    fn insert(&mut self, field: &Field, value: impl Into<Value>) {
        let name = field.name();
        let value = if is_secret_field(name) {
            REDACTED.into()
        } else {
            value.into()
        };
        self.0.insert(name.to_owned(), value);
    }
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.insert(field, format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field, value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field, value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field, value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field, value);
    }
}

/// The lines a [`JournalLayer`] wrote, parsed; clones share them.
#[derive(Clone, Debug, Default)]
pub struct Lines(Arc<Mutex<Vec<Value>>>);

impl Lines {
    /// Adds a line.
    fn push(&self, value: Value) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(value);
    }

    /// The lines written so far, leaving none behind.
    pub fn take(&self) -> Vec<Value> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use tracing::{info, info_span, warn};
    use tracing_subscriber::layer::SubscriberExt;

    /// Runs `report` with a journal and returns the text and parsed lines.
    fn journal(report: impl FnOnce()) -> (String, Vec<Value>) {
        let buffer = Buffer::default();
        let layer = JournalLayer::new(buffer.clone());
        let lines = layer.lines();
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, report);
        (buffer.text(), lines.take())
    }

    /// A writer whose clones share their bytes.
    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Buffer {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn an_event_becomes_one_line_with_its_spans_outermost_first() {
        let (_, lines) = journal(|| {
            let _unlock = info_span!("unlock", device = "/dev/sda2").entered();
            let _method = info_span!("key_method", method = "key file").entered();
            info!(name = "cryptroot", "opened");
        });

        assert_eq!(
            lines,
            [json!({
                "pid": std::process::id(),
                "seq": 0,
                "level": "INFO",
                "target": "zbm_trace::journal::tests",
                "message": "opened",
                "fields": { "name": "cryptroot" },
                "spans": [
                    { "span": "unlock", "device": "/dev/sda2" },
                    { "span": "key_method", "method": "key file" },
                ],
            })]
        );
    }

    #[test]
    fn lines_are_numbered_and_written_as_json_lines() {
        let (text, _) = journal(|| {
            info!("first");
            warn!(code = 4, "second");
        });

        let parsed: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(parsed[0]["seq"], 0);
        assert_eq!(parsed[1]["seq"], 1);
        assert_eq!(parsed[1]["level"], "WARN");
        assert_eq!(parsed[1]["fields"]["code"], 4);
    }

    #[test]
    fn fields_recorded_later_appear_in_the_span() {
        let (_, lines) = journal(|| {
            let span = info_span!("import", pool = "tank", status = tracing::field::Empty);
            span.record("status", "ONLINE");
            let _entered = span.entered();
            info!("imported");
        });

        assert_eq!(lines[0]["spans"][0]["status"], "ONLINE");
    }

    #[test]
    fn debug_values_are_written_as_their_debug_text() {
        struct Secretish;
        impl fmt::Debug for Secretish {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("Secret(..)")
            }
        }

        let (text, _) = journal(|| info!(key = ?Secretish, "read"));

        assert!(text.contains("\"key\":\"Secret(..)\""));
    }

    #[test]
    fn fields_named_for_secrets_are_redacted_whatever_their_type() {
        let (text, lines) = journal(|| {
            let _span = info_span!("prompt", passphrase = "hunter2").entered();
            info!(pin = 1234, yubikey_response = "abcd", pool = "tank", "read");
        });

        assert!(!text.contains("hunter2") && !text.contains("1234") && !text.contains("abcd"));
        assert_eq!(lines[0]["fields"]["pin"], REDACTED);
        assert_eq!(lines[0]["fields"]["yubikey_response"], REDACTED);
        assert_eq!(lines[0]["fields"]["pool"], "tank");
        assert_eq!(lines[0]["spans"][0]["passphrase"], REDACTED);
    }

    #[test]
    fn secret_field_names_are_whole_words() {
        let cases = [
            ("pin", true),
            ("tpm2_pin", true),
            ("passphrase", true),
            ("key_passphrase", true),
            ("pinned", false),
            ("spin", false),
            ("pool", false),
            ("response_ms", false),
        ];

        for (name, secret) in cases {
            assert_eq!(is_secret_field(name), secret, "{name}");
        }
    }

    #[test]
    fn a_full_journal_says_so_once_and_takes_no_more_lines() {
        let buffer = Buffer::default();
        let layer = JournalLayer::new(buffer.clone()).with_limit(200);
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            for count in 0..10 {
                info!(count, "line");
            }
        });

        let text = buffer.text();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.len() < 10, "{text}");
        let last: Value = serde_json::from_str(lines.last().unwrap()).unwrap();
        assert_eq!(last["message"], FULL);
        assert_eq!(text.matches(FULL).count(), 1);
    }
}
