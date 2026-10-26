//! The kernel log in the formats of the legacy runtime.
//!
//! Messages for the user's kernel log are `tracing` events with the target
//! [`KMSG`] (or [`KMSG_NOTICE`]), which this crate's macros give them:
//! [`error!`](crate::error), [`warn!`](crate::warn),
//! [`notice!`](crate::notice), [`info!`](crate::info) and
//! [`debug!`](crate::debug). [`KmsgLayer`] writes them as
//! `kmsg-log-lib.sh` does: `<N>ZFSBootMenu: text`, debugging output as
//! `<7>ZBM:[pid]|file,line|text` line by line, filtered by `loglevel`, with the
//! marker files `have_warnings` and `have_errors`. Other events, such as the
//! command journal's, never reach the kernel log.
//!
//! ```
//! use std::sync::{Arc, Mutex};
//! use zbm_trace::{KmsgLayer, KmsgSink};
//! use tracing_subscriber::layer::SubscriberExt;
//!
//! #[derive(Clone, Default)]
//! struct Lines(Arc<Mutex<Vec<String>>>);
//! impl KmsgSink for Lines {
//!     fn record(&self, line: &str) {
//!         self.0.lock().unwrap().push(line.to_owned());
//!     }
//!     fn mark(&self, _marker: &str) {}
//! }
//!
//! let lines = Lines::default();
//! let layer = KmsgLayer::new(lines.clone(), "6", 1);
//! let subscriber = tracing_subscriber::registry().with(layer);
//! tracing::subscriber::with_default(subscriber, || {
//!     zbm_trace::info!("importing {}", "tank");
//!     zbm_trace::debug!("not shown at loglevel 6");
//!     tracing::info!("only for the journal");
//! });
//! assert_eq!(*lines.0.lock().unwrap(), ["<6>ZFSBootMenu: importing tank\n"]);
//! ```

use std::fmt;
use std::panic::Location;

use tracing::field::{Field, Visit};
use tracing::metadata::Kind;
use tracing::subscriber::Interest;
use tracing::{Event, Level as TracingLevel, Metadata, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// The target of events for the kernel log.
pub const KMSG: &str = "zbm";

/// The target of notices for the kernel log: `tracing` has no notice level,
/// so a notice is an `INFO` event with this target.
pub const KMSG_NOTICE: &str = "zbm::notice";

/// The target of the question whether a level is logged ([asked
/// here](enabled)): [`KmsgLayer`] answers it by its level, while it lets the
/// events of [`KMSG`] through to every layer, so that the journal records the
/// messages the kernel log leaves out.
const PROBE: &str = "zbm::probe";

/// The target of the question whether notices are logged.
const PROBE_NOTICE: &str = "zbm::probe::notice";

/// The level of a message, with the numbers of the kernel log.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Level {
    /// An error (level 3).
    Error,
    /// A warning (level 4).
    Warning,
    /// A notice (level 5).
    Notice,
    /// Information (level 6).
    Info,
    /// Debugging output (level 7).
    Debug,
}

impl Level {
    /// The kernel log level.
    pub fn number(self) -> i64 {
        match self {
            Level::Error => 3,
            Level::Warning => 4,
            Level::Notice => 5,
            Level::Info => 6,
            Level::Debug => 7,
        }
    }

    /// The label `echo-log-lib.sh` prints before a message on standard error.
    pub fn label(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warning => "WARN",
            Level::Notice => "NOTICE",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
        }
    }

    /// The level of an event of the kernel log, from its metadata.
    fn of(metadata: &Metadata<'_>) -> Level {
        match *metadata.level() {
            TracingLevel::ERROR => Level::Error,
            TracingLevel::WARN => Level::Warning,
            TracingLevel::INFO if matches!(metadata.target(), KMSG_NOTICE | PROBE_NOTICE) => {
                Level::Notice
            }
            TracingLevel::INFO => Level::Info,
            TracingLevel::DEBUG | TracingLevel::TRACE => Level::Debug,
        }
    }
}

/// A message collected to be logged later, such as the messages of parsing
/// the command line, which come before the log is set up.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message {
    /// The level.
    pub level: Level,
    /// The text.
    pub text: String,
}

impl Message {
    /// A message with the given level.
    pub fn new(level: Level, text: impl Into<String>) -> Message {
        Message {
            level,
            text: text.into(),
        }
    }

    /// An error.
    pub fn error(text: impl Into<String>) -> Message {
        Message::new(Level::Error, text)
    }

    /// A warning.
    pub fn warning(text: impl Into<String>) -> Message {
        Message::new(Level::Warning, text)
    }

    /// A notice.
    pub fn notice(text: impl Into<String>) -> Message {
        Message::new(Level::Notice, text)
    }

    /// Information.
    pub fn info(text: impl Into<String>) -> Message {
        Message::new(Level::Info, text)
    }

    /// Debugging output.
    pub fn debug(text: impl Into<String>) -> Message {
        Message::new(Level::Debug, text)
    }

    /// The message as `echo-log-lib.sh` prints it: `LEVEL: text`.
    pub fn stderr_line(&self) -> String {
        format!("{}: {}", self.level.label(), self.text)
    }

    /// Sends the message to the kernel log. Debugging output names the caller
    /// of this method, as it would name the caller of [`debug!`](crate::debug).
    #[track_caller]
    pub fn emit(&self) {
        let caller = Location::caller();
        let (file, line, text) = (caller.file(), caller.line(), self.text.as_str());
        match self.level {
            Level::Error => tracing::error!(target: KMSG, file, line, "{text}"),
            Level::Warning => tracing::warn!(target: KMSG, file, line, "{text}"),
            Level::Notice => tracing::info!(target: KMSG_NOTICE, file, line, "{text}"),
            Level::Info => tracing::info!(target: KMSG, file, line, "{text}"),
            Level::Debug => tracing::debug!(target: KMSG, file, line, "{text}"),
        }
    }
}

/// Whether the kernel log takes messages of `level`, so that debugging output
/// that is costly to gather can be left out.
///
/// The question is asked of a target of its own, which [`KmsgLayer`] judges
/// by its level; without a kernel log, nothing is taken.
pub fn enabled(level: Level) -> bool {
    const EVENT: Kind = Kind::EVENT;
    match level {
        Level::Error => tracing::enabled!(kind: EVENT, target: PROBE, TracingLevel::ERROR),
        Level::Warning => tracing::enabled!(kind: EVENT, target: PROBE, TracingLevel::WARN),
        Level::Notice => {
            tracing::enabled!(kind: EVENT, target: PROBE_NOTICE, TracingLevel::INFO)
        }
        Level::Info => tracing::enabled!(kind: EVENT, target: PROBE, TracingLevel::INFO),
        Level::Debug => tracing::enabled!(kind: EVENT, target: PROBE, TracingLevel::DEBUG),
    }
}

/// The log level when none is set, as `${loglevel:-4}` gives it.
pub const DEFAULT_LEVEL: i64 = 4;

/// The log level as the legacy code reads it, `${loglevel:-4}`: 4 when it
/// is empty or not set; else an integer operand of `test`, or `None` when it
/// is not one, in which case nothing is logged.
pub fn parse_level(loglevel: &str) -> Option<i64> {
    let loglevel = loglevel.trim();
    if loglevel.is_empty() {
        return Some(DEFAULT_LEVEL);
    }
    loglevel.parse().ok()
}

/// The kernel log lines of a message, as `kmsg-log-lib.sh` writes them.
///
/// Debugging output is written line by line: every line starts with
/// `<7>ZBM:[pid]|`, and the last line carries the call site between the bars
/// (`<7>ZBM:[pid]|trace|line`). Other levels are written as `<N>ZFSBootMenu:
/// text`.
///
/// The kernel refuses a record longer than 992 bytes (`LOG_LINE_MAX`), so
/// longer text, such as a command's output, is cut into several records of
/// at most [`RECORD`] bytes, each with the prefix.
pub fn kmsg_lines(level: Level, text: &str, pid: u32, trace: &str) -> Vec<String> {
    if level != Level::Debug {
        let prefix = format!("<{}>ZFSBootMenu: ", level.number());
        return pieces(text, RECORD.saturating_sub(prefix.len()))
            .map(|piece| format!("{prefix}{piece}"))
            .collect();
    }
    let mut lines = text.split('\n').peekable();
    let mut records = Vec::new();
    while let Some(line) = lines.next() {
        let last_line = lines.peek().is_none();
        let room = RECORD.saturating_sub(format!("<7>ZBM:[{pid}]|{trace}|").len());
        let mut parts = pieces(line, room).peekable();
        while let Some(part) = parts.next() {
            let trace = if last_line && parts.peek().is_none() {
                trace
            } else {
                ""
            };
            records.push(format!("<7>ZBM:[{pid}]|{trace}|{part}"));
        }
    }
    records
}

/// The longest record written to the kernel log, prefix included: below
/// the kernel's 992 bytes.
pub const RECORD: usize = 960;

/// `text` in pieces of at most `size` bytes, cut between characters; one
/// empty piece for no text.
fn pieces(text: &str, size: usize) -> impl Iterator<Item = &str> {
    let size = size.max(4);
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let text = rest?;
        if text.len() <= size {
            rest = None;
            return Some(text);
        }
        let mut cut = size;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        let (piece, after) = text.split_at(cut);
        rest = Some(after);
        Some(piece)
    })
}

/// Where [`KmsgLayer`] writes: the kernel log device and the state directory.
pub trait KmsgSink: Send + Sync + 'static {
    /// Writes one record to the kernel log. Failures are ignored, as
    /// `echo ... > /dev/kmsg` ignores them.
    fn record(&self, line: &str);

    /// Creates the marker file `marker` (`have_warnings` or `have_errors`) in
    /// the state directory, ignoring failures.
    fn mark(&self, marker: &str);
}

/// A `tracing` layer that writes the events of [`KMSG`] and [`KMSG_NOTICE`]
/// to the kernel log, in the formats of the legacy runtime.
pub struct KmsgLayer<S> {
    /// Where the lines and markers go.
    sink: S,
    /// The log level; nothing is written when it is not an integer.
    loglevel: Option<i64>,
    /// The process ID that debugging output names.
    pid: u32,
    /// Whether every message is written as one `LEVEL: text` line, whatever
    /// its level.
    labelled: bool,
}

impl<S: KmsgSink> KmsgLayer<S> {
    /// A layer writing to `sink` the messages that `loglevel` (as
    /// [`parse_level`] reads it) allows, naming process `pid` in debugging
    /// output.
    pub fn new(sink: S, loglevel: &str, pid: u32) -> KmsgLayer<S> {
        KmsgLayer {
            sink,
            loglevel: parse_level(loglevel),
            pid,
            labelled: false,
        }
    }

    /// The same layer, writing each message of any level as one `LEVEL: text`
    /// line, as the logging stand-ins of the LUKS unit suites do; empty
    /// debugging output is left out.
    #[must_use]
    pub fn labelled(mut self) -> KmsgLayer<S> {
        self.labelled = true;
        self
    }

    /// Whether messages of `level` are written.
    fn allows(&self, level: Level) -> bool {
        self.labelled
            || self
                .loglevel
                .is_some_and(|loglevel| loglevel >= level.number())
    }

    /// Writes `message`, whose debugging output names `trace`.
    fn write(&self, message: &Message, trace: &str) {
        let marker = match message.level {
            Level::Warning => Some("have_warnings"),
            Level::Error => Some("have_errors"),
            Level::Notice | Level::Info | Level::Debug => None,
        };
        if let Some(marker) = marker {
            self.sink.mark(marker);
        }
        if self.labelled {
            if message.level != Level::Debug || !message.text.is_empty() {
                self.sink.record(&format!("{}\n", message.stderr_line()));
            }
            return;
        }
        // The kernel makes one record of each write.
        for line in kmsg_lines(message.level, &message.text, self.pid, trace) {
            self.sink.record(&format!("{line}\n"));
        }
    }
}

/// Whether `metadata` is of an event for the kernel log.
fn is_kmsg(metadata: &Metadata<'_>) -> bool {
    metadata.is_event() && matches!(metadata.target(), KMSG | KMSG_NOTICE)
}

/// Whether `metadata` is of the question whether a level is logged.
fn is_probe(metadata: &Metadata<'_>) -> bool {
    matches!(metadata.target(), PROBE | PROBE_NOTICE)
}

impl<S: KmsgSink, R: Subscriber> Layer<R> for KmsgLayer<S> {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        // Each thread can have a log with its own level, so the answer to
        // `enabled` cannot be cached: it is asked each time. Everything else
        // is left to the other layers.
        if is_probe(metadata) {
            Interest::sometimes()
        } else {
            Interest::always()
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>, _context: Context<'_, R>) -> bool {
        // Disabling an event here would hide it from every layer, the journal
        // too, so only the question of `enabled` is judged; `on_event` leaves
        // out the messages the level does not allow.
        !is_probe(metadata) || self.allows(Level::of(metadata))
    }

    fn on_event(&self, event: &Event<'_>, _context: Context<'_, R>) {
        let metadata = event.metadata();
        if !is_kmsg(metadata) {
            return;
        }
        let level = Level::of(metadata);
        if !self.allows(level) {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let file = fields
            .file
            .unwrap_or_else(|| metadata.file().unwrap_or_default().to_owned());
        let line = fields.line.or(metadata.line().map(u64::from));
        let trace = format!("{file},{}", line.unwrap_or_default());
        self.write(&Message::new(level, fields.message), &trace);
    }
}

/// The fields of an event the kernel log uses.
#[derive(Default)]
struct Fields {
    /// The message.
    message: String,
    /// The caller's file, when [`Message::emit`] gives it.
    file: Option<String>,
    /// The caller's line, when [`Message::emit`] gives it.
    line: Option<u64>,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "file" => self.file = Some(value.to_owned()),
            "message" => value.clone_into(&mut self.message),
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "line" {
            self.line = Some(value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        }
    }
}

/// Logs an error to the kernel log, with `format!` arguments.
#[macro_export]
macro_rules! error {
    ($($arg:tt)+) => {
        $crate::__tracing::error!(target: $crate::KMSG, $($arg)+)
    };
}

/// Logs a warning to the kernel log, with `format!` arguments.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)+) => {
        $crate::__tracing::warn!(target: $crate::KMSG, $($arg)+)
    };
}

/// Logs a notice to the kernel log, with `format!` arguments.
#[macro_export]
macro_rules! notice {
    ($($arg:tt)+) => {
        $crate::__tracing::info!(target: $crate::KMSG_NOTICE, $($arg)+)
    };
}

/// Logs information to the kernel log, with `format!` arguments.
#[macro_export]
macro_rules! info {
    ($($arg:tt)+) => {
        $crate::__tracing::info!(target: $crate::KMSG, $($arg)+)
    };
}

/// Logs debugging output to the kernel log, with `format!` arguments; it
/// names the file and line of the call.
#[macro_export]
macro_rules! debug {
    ($($arg:tt)+) => {
        $crate::__tracing::debug!(target: $crate::KMSG, $($arg)+)
    };
}

#[cfg(test)]
mod tests;
