//! Tracing for ZFSBootMenu: what happened, during which stage, for which
//! volume or pool, and which programs ran.
//!
//! Code reports with the [`tracing`] macros: a span for each operation
//! (`info_span!("unlock", device = %device)`) and events inside it. This
//! crate decides where those go:
//!
//! - [`JournalLayer`] writes every event as one line of JSON, with the fields
//!   of the spans it happened in, so a failed boot can be read back step by
//!   step (`docs/dev/testing.md`, "When a test fails").
//! - [`KmsgLayer`] writes the events meant for the user's kernel log in the
//!   formats of the legacy runtime (`<6>ZFSBootMenu: text`); this crate's
//!   macros, [`error!`], [`warn!`], [`notice!`], [`info!`] and [`debug!`],
//!   emit them ([`kmsg`]).
//! - [`install`] sets up a program: messages of a chosen level on standard
//!   error, and the journal if one is given.
//! - [`crash`] writes and reads crash reports, with the spans that were open
//!   ([`crash::SpanStack`]).
//!
//! The command journal, one event per program run with its duration and
//! status, comes from `zbm-platform`, whose runner reports each command.
//!
//! Secrets never reach the journal: `Secret` prints as `Secret(..)`, and the
//! rule of `crates/DESIGN.md` keeps them out of arguments.
//!
//! ```
//! use tracing::{info, info_span};
//! use zbm_trace::JournalLayer;
//! use tracing_subscriber::layer::SubscriberExt;
//!
//! let journal = JournalLayer::new(Vec::new()).with_copy();
//! let lines = journal.lines();
//! let subscriber = tracing_subscriber::registry().with(journal);
//! tracing::subscriber::with_default(subscriber, || {
//!     let _unlock = info_span!("unlock", device = "/dev/sda2").entered();
//!     info!(method = "key file", "opened");
//! });
//!
//! let line = &lines.take()[0];
//! assert_eq!(line["message"], "opened");
//! assert_eq!(line["spans"][0]["device"], "/dev/sda2");
//! ```

pub mod crash;
mod journal;
pub mod kmsg;
mod template;

pub use journal::{
    JournalLayer, KEPT_FIELDS, LIMIT as JOURNAL_LIMIT, Lines, REDACTED, is_kept_field,
    is_secret_field,
};
pub use kmsg::{KMSG, KMSG_NOTICE, KmsgLayer, KmsgSink, Level, Message, enabled};
pub use template::TEMPLATE;
pub use tracing::level_filters::LevelFilter;

/// `tracing`, for the macros of [`kmsg`], so that their callers need not
/// depend on it.
#[doc(hidden)]
pub use tracing as __tracing;

use std::io::Write;

use tracing_subscriber::Layer as _;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// Why [`install`] failed.
#[derive(Debug, thiserror::Error)]
#[error("a tracing subscriber is already installed")]
pub struct InstallError(#[from] tracing_subscriber::util::TryInitError);

/// Where a program's traces go.
pub struct Config {
    /// Events of this level and more severe are printed on standard error.
    pub stderr: LevelFilter,
    /// Every event is written here as JSON, if set.
    pub journal: Option<Box<dyn Write + Send>>,
}

/// The fields of events on standard error, as the default format writes them
/// (`name=value`, the message alone), but with the value of a field that is
/// not kept ([`is_kept_field`]) replaced by [`REDACTED`], as in the journal.
fn redacted_fields() -> impl for<'w> tracing_subscriber::fmt::FormatFields<'w> + 'static {
    use tracing_subscriber::field::MakeExt as _;
    tracing_subscriber::fmt::format::debug_fn(|writer, field, value| {
        if field.name() == "message" {
            write!(writer, "{value:?}")
        } else if is_kept_field(field.name()) {
            write!(writer, "{field}={value:?}")
        } else {
            write!(writer, "{field}={REDACTED}")
        }
    })
    .delimited(" ")
}

/// Installs the subscriber of a program for the rest of its life.
///
/// # Errors
///
/// [`InstallError`] when a subscriber is installed already.
pub fn install(config: Config) -> Result<(), InstallError> {
    let stderr = tracing_subscriber::fmt::layer()
        .fmt_fields(redacted_fields())
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(false)
        .with_filter(config.stderr);
    tracing_subscriber::registry()
        .with(stderr)
        .with(
            config
                .journal
                .map(|journal| JournalLayer::new(journal).with_limit(journal::LIMIT)),
        )
        .try_init()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::layer::SubscriberExt as _;

    use super::*;

    /// Lines written to standard error, kept for the test.
    #[derive(Clone, Default)]
    struct Kept(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Kept {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn fields_named_for_secrets_are_redacted_on_standard_error() {
        let kept = Kept::default();
        let writer = kept.clone();
        let layer = tracing_subscriber::fmt::layer()
            .fmt_fields(redacted_fields())
            .with_writer(move || writer.clone())
            .without_time()
            .with_ansi(false);
        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(passphrase = "hunter2", device = "/dev/sda", "wrong key");
        });

        let text = String::from_utf8(kept.0.lock().unwrap().clone()).unwrap();
        assert!(!text.contains("hunter2"), "{text}");
        assert!(text.contains(&format!("passphrase={REDACTED}")), "{text}");
        assert!(text.contains("wrong key passphrase="), "{text}");
        assert!(text.contains("device=\"/dev/sda\""), "{text}");
    }
}
