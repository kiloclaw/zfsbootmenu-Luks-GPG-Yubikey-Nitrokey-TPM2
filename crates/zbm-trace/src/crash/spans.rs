//! The spans each thread is in, so that a crash report can name them.

use std::cell::RefCell;
use std::fmt::Write as _;

use tracing::span::{Attributes, Id, Record};
use tracing::{Subscriber, field::Field, field::Visit};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::journal::{REDACTED, is_secret_field};

thread_local! {
    /// The spans this thread entered and has not left, outermost first.
    static OPEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The spans the calling thread is in, outermost first, each as
/// `name{field=value, ...}` with secrets redacted as in the journal; empty
/// unless a [`SpanStack`] is installed.
pub fn open_spans() -> Vec<String> {
    OPEN.with(|open| {
        open.try_borrow()
            .map(|open| open.clone())
            .unwrap_or_default()
    })
}

/// A `tracing` layer that keeps track of the spans each thread is in, for
/// [`open_spans`].
#[derive(Debug, Default)]
pub struct SpanStack;

/// A span's name and fields as text, kept in the span's extensions.
struct Described(String);

/// Collects fields as `name=value` pairs.
#[derive(Default)]
struct Pairs(Vec<String>);

impl Visit for Pairs {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let mut pair = format!("{}=", field.name());
        if is_secret_field(field.name()) {
            pair.push_str(REDACTED);
        } else {
            let _ = write!(pair, "{value:?}");
        }
        self.0.push(pair);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_debug(field, &format_args!("{value}"));
    }
}

/// `name{a=1, b=2}`, or `name` without fields.
fn describe(name: &str, pairs: &[String]) -> String {
    if pairs.is_empty() {
        name.to_owned()
    } else {
        format!("{name}{{{}}}", pairs.join(", "))
    }
}

impl<S> tracing_subscriber::Layer<S> for SpanStack
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        let mut pairs = Pairs::default();
        attributes.record(&mut pairs);
        if let Some(span) = context.span(id) {
            let text = describe(span.name(), &pairs.0);
            span.extensions_mut().insert((Described(text), pairs));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, context: Context<'_, S>) {
        if let Some(span) = context.span(id) {
            let name = span.name();
            let mut extensions = span.extensions_mut();
            if let Some((described, pairs)) = extensions.get_mut::<(Described, Pairs)>() {
                values.record(pairs);
                described.0 = describe(name, &pairs.0);
            }
        }
    }

    fn on_enter(&self, id: &Id, context: Context<'_, S>) {
        let text = context.span(id).map_or_else(String::new, |span| {
            span.extensions().get::<(Described, Pairs)>().map_or_else(
                || span.name().to_owned(),
                |(described, _)| described.0.clone(),
            )
        });
        OPEN.with(|open| {
            if let Ok(mut open) = open.try_borrow_mut() {
                open.push(text);
            }
        });
    }

    fn on_exit(&self, _id: &Id, _context: Context<'_, S>) {
        OPEN.with(|open| {
            if let Ok(mut open) = open.try_borrow_mut() {
                open.pop();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tracing::info_span;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn open_spans_are_named_with_their_fields_and_left_spans_are_gone() {
        let subscriber = tracing_subscriber::registry().with(SpanStack);
        tracing::subscriber::with_default(subscriber, || {
            let _startup = info_span!("startup").entered();
            let import = info_span!("import", pool = "tank", status = tracing::field::Empty);
            import.record("status", "ONLINE");
            let inside = import.in_scope(|| {
                let _unlock = info_span!("unlock", device = "/dev/sda2", pin = "1234").entered();
                open_spans()
            });

            assert_eq!(
                inside,
                [
                    "startup",
                    "import{pool=tank, status=ONLINE}",
                    "unlock{device=/dev/sda2, pin=[redacted]}",
                ]
            );
            assert_eq!(open_spans(), ["startup"]);
        });

        assert_eq!(open_spans(), Vec::<String>::new());
    }
}
