//! Messages as constant templates, with their values as fields:
//! `zbm_trace::debug!("pool set to {pool}"; %pool)`. The template names the
//! event, so the journal can be searched for it; the kernel log and the
//! journal's message show it with each `{name}` replaced by the value of the
//! field `name`, as a format string would have shown it.

use std::fmt;

use tracing::field::{Field, Visit};

/// The name of the field that holds the template.
pub const TEMPLATE: &str = "template";

/// The values of an event's fields, as text, for rendering its template.
#[derive(Debug, Default)]
pub(crate) struct Values {
    /// The template, when the event has one.
    pub(crate) template: Option<String>,
    values: Vec<(&'static str, String)>,
}

impl Values {
    /// The value of the field `name`.
    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(field, _)| *field == name)
            .map(|(_, value)| value.as_str())
    }

    /// The message: the template with its fields' values, if there is one.
    pub(crate) fn message(&self) -> Option<String> {
        self.template
            .as_deref()
            .map(|template| render(template, self))
    }

    fn insert(&mut self, field: &Field, value: String) {
        if field.name() == TEMPLATE {
            self.template = Some(value);
        } else {
            self.values.push((field.name(), value));
        }
    }
}

impl Visit for Values {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field, value.to_owned());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field, value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field, value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field, value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        // `%value` records the `Display` text through `Debug`.
        self.insert(field, format!("{value:?}"));
    }
}

/// `template` with each `{name}` replaced by the value of the field `name`,
/// and `{{` and `}}` by single braces. A name without a field stays as it
/// is, so that a mistake shows in the log; the metrics refuse one (`xtask
/// metrics`, `fmt logs`).
fn render(template: &str, values: &Values) -> String {
    let mut text = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find(['{', '}']) {
        let (before, after) = rest.split_at(at);
        text.push_str(before);
        if let Some(tail) = after.strip_prefix("{{") {
            text.push('{');
            rest = tail;
        } else if let Some(tail) = after.strip_prefix("}}") {
            text.push('}');
            rest = tail;
        } else if let Some(close) = after.find('}').filter(|_| after.starts_with('{')) {
            let name = after.get(1..close).unwrap_or_default();
            match values.get(name) {
                Some(value) => text.push_str(value),
                None => text.push_str(after.get(..=close).unwrap_or_default()),
            }
            rest = after.get(close + 1..).unwrap_or_default();
        } else {
            text.push_str(after.get(..1).unwrap_or_default());
            rest = after.get(1..).unwrap_or_default();
        }
    }
    text.push_str(rest);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&'static str, &str)]) -> Values {
        Values {
            template: None,
            values: pairs.iter().map(|(n, v)| (*n, (*v).to_owned())).collect(),
        }
    }

    #[test]
    fn names_are_replaced_by_their_values() {
        let pool = values(&[("pool", "tank"), ("code", "1")]);
        assert_eq!(render("pool set to {pool}", &pool), "pool set to tank");
        assert_eq!(render("{pool}: {code}, {pool}", &pool), "tank: 1, tank");
        assert_eq!(render("{{literal}} {pool}}}", &pool), "{literal} tank}");
        assert_eq!(render("{missing} stays", &pool), "{missing} stays");
        assert_eq!(render("a } and { alone", &pool), "a } and { alone");
        assert_eq!(render("", &pool), "");
    }
}
