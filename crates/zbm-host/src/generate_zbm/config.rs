//! The `generate-zbm` configuration file (`config.yaml`).
//!
//! Values keep what the file says (ADR 0013): `null`, `~` and nothing are
//! undefined, `true` and `false` are booleans, and every other scalar is
//! its text as written, so `Version: 3.10` is `3.10` and a number is read
//! as one only where a key wants a number ([`Value::integer`]). Booleans
//! are read as the manual says ([`Value::truthy`]): `false` is false, and so
//! are `no`, `off` and `0`.
//!
//! Sequences and mappings are shared: an alias to an anchored mapping is the
//! same mapping, so a change made through one name is seen through the
//! other.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle};
use zbm_config::spec::OptionSpec;

/// A sequence, shared like a Perl array reference.
pub type Seq = Rc<RefCell<Vec<Value>>>;

/// A mapping in file order, shared like a Perl hash reference.
pub type Map = Rc<RefCell<Vec<(String, Value)>>>;

/// The section and the key of a configuration key of the table, whose name
/// is `Section.Key`.
fn section_key(spec: &OptionSpec) -> (&'static str, &'static str) {
    spec.name.split_once('.').unwrap_or(("", spec.name))
}

/// A configuration value. Cloning a sequence or a mapping shares it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// An undefined value: `null`, `~` or nothing.
    Null,
    /// A boolean (`true`, `False` and so on).
    Bool(bool),
    /// Any other scalar, as written.
    Text(String),
    /// A sequence.
    Seq(Seq),
    /// A mapping, in file order.
    Map(Map),
}

/// An error that ends `generate-zbm` with the message on standard error
/// and exit status 255.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fatal(pub String);

impl Value {
    /// A new sequence.
    pub fn seq(items: Vec<Value>) -> Value {
        Value::Seq(Rc::new(RefCell::new(items)))
    }

    /// A new mapping.
    pub fn map(entries: Vec<(String, Value)>) -> Value {
        Value::Map(Rc::new(RefCell::new(entries)))
    }

    /// Whether the value is defined.
    pub fn is_defined(&self) -> bool {
        !matches!(self, Value::Null)
    }

    /// The scalar as text, `true` or `false` for a boolean; `None` when it
    /// is undefined or a collection.
    pub fn text(&self) -> Option<String> {
        match self {
            Value::Null | Value::Seq(_) | Value::Map(_) => None,
            Value::Bool(value) => Some(value.to_string()),
            Value::Text(text) => Some(text.clone()),
        }
    }

    /// Whether the value is defined and not empty: a boolean always is.
    pub fn has_value(&self) -> bool {
        self.text().is_some_and(|text| !text.is_empty())
    }

    /// Whether the value says yes: a true boolean, or any text but the
    /// empty one, `0`, `false`, `no` and `off` (in any case); a collection
    /// is true.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(value) => *value,
            Value::Text(text) => !matches!(
                text.trim().to_ascii_lowercase().as_str(),
                "" | "0" | "false" | "no" | "off"
            ),
            Value::Seq(_) | Value::Map(_) => true,
        }
    }

    /// The value as a whole number: a decimal, octal (`0o`) or hexadecimal
    /// (`0x`) integer, or a decimal number without its fraction; `None`
    /// for anything else, such as a word or a boolean.
    pub fn integer(&self) -> Option<i64> {
        let Value::Text(text) = self else {
            return None;
        };
        let text = text.trim();
        if let Some(digits) = text.strip_prefix("0o") {
            return i64::from_str_radix(digits, 8).ok();
        }
        if let Some(digits) = text.strip_prefix("0x") {
            return i64::from_str_radix(digits, 16).ok();
        }
        if let Ok(number) = text.parse::<i64>() {
            return Some(number);
        }
        let number: f64 = text.parse().ok()?;
        // Truncated toward zero, and saturating at the range
        number.is_finite().then(|| number.trunc() as i64)
    }

    /// The value of a mapping's key, if this is a mapping that has it.
    pub fn get(&self, key: &str) -> Option<Value> {
        match self {
            Value::Map(map) => map
                .borrow()
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone()),
            _ => None,
        }
    }

    /// The value of the configuration key `spec` of the [table of
    /// options](zbm_config::spec::generate), `Section.Key`, as
    /// [`Value::lookup`] reads it.
    ///
    /// # Errors
    ///
    /// [`Fatal`] when the section is defined but is not a mapping.
    pub fn option(&self, spec: &OptionSpec) -> Result<Option<Value>, Fatal> {
        let (section, key) = section_key(spec);
        self.lookup(section, key)
    }

    /// The defined value of `key` in `section`, or `None` when either is
    /// missing.
    ///
    /// # Errors
    ///
    /// [`Fatal`] when the section is defined but is not a mapping.
    pub fn lookup(&self, section: &str, key: &str) -> Result<Option<Value>, Fatal> {
        match self.get(section) {
            None | Some(Value::Null) => Ok(None),
            Some(map @ Value::Map(_)) => Ok(map.get(key).filter(Value::is_defined)),
            Some(_) => Err(not_a_mapping(section)),
        }
    }
}

/// The error for a section that is not a mapping.
fn not_a_mapping(section: &str) -> Fatal {
    Fatal(format!(
        "The section {section} of the configuration is not a mapping of keys"
    ))
}

/// Types a plain scalar: null, boolean, or text.
pub fn resolve_plain(text: &str) -> Value {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => Value::Null,
        "true" | "True" | "TRUE" => Value::Bool(true),
        "false" | "False" | "FALSE" => Value::Bool(false),
        _ => Value::Text(text.to_owned()),
    }
}

/// Why the configuration could not be read; `generate-zbm` reports it as
/// "Unable to parse configuration".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadError(pub String);

#[derive(Default)]
struct Events(Vec<Event>);

impl MarkedEventReceiver for Events {
    fn on_event(&mut self, event: Event, _mark: Marker) {
        self.0.push(event);
    }
}

/// Reads a configuration file's text. Only the first document counts, as when
/// YAML::PP loads into a scalar, and it must be a mapping.
pub fn load(text: &str) -> Result<Value, LoadError> {
    let mut events = Events::default();
    Parser::new_from_str(text)
        .load(&mut events, false)
        .map_err(|error| LoadError(error.to_string()))?;
    let mut builder = Builder {
        events: events.0.into_iter(),
        anchors: HashMap::new(),
    };
    let document = builder.document()?;
    match document {
        Some(map @ Value::Map(_)) => Ok(map),
        Some(_) => Err(LoadError("the configuration is not a mapping".to_owned())),
        None => Err(LoadError("the configuration is empty".to_owned())),
    }
}

/// Builds values from parser events.
struct Builder {
    events: std::vec::IntoIter<Event>,
    anchors: HashMap<usize, Value>,
}

impl Builder {
    /// The first document's value, or `None` when the stream has no document.
    fn document(&mut self) -> Result<Option<Value>, LoadError> {
        loop {
            match self.events.next() {
                None | Some(Event::StreamEnd) => return Ok(None),
                Some(Event::DocumentStart) => {
                    return match self.events.next() {
                        Some(Event::DocumentEnd) | None => Ok(Some(Value::Null)),
                        Some(event) => self.node(event).map(Some),
                    };
                }
                Some(_) => {}
            }
        }
    }

    fn node(&mut self, event: Event) -> Result<Value, LoadError> {
        let (value, anchor) = match event {
            Event::Scalar(text, style, anchor, tag) => (scalar(&text, style, tag.as_ref()), anchor),
            Event::Alias(anchor) => {
                // An anchor is registered once its node is complete, so an alias inside its own
                // anchor is unknown here; YAML::PP also refuses such cyclic aliases.
                return self
                    .anchors
                    .get(&anchor)
                    .cloned()
                    .ok_or_else(|| LoadError(format!("unknown or cyclic alias {anchor}")));
            }
            Event::SequenceStart(anchor, _) => {
                let mut items = Vec::new();
                loop {
                    match self.events.next() {
                        Some(Event::SequenceEnd) => break,
                        Some(event) => items.push(self.node(event)?),
                        None => return Err(LoadError("unterminated sequence".to_owned())),
                    }
                }
                (Value::seq(items), anchor)
            }
            Event::MappingStart(anchor, _) => {
                let mut entries: Vec<(String, Value)> = Vec::new();
                loop {
                    let key = match self.events.next() {
                        Some(Event::MappingEnd) => break,
                        Some(event) => self.node(event)?,
                        None => return Err(LoadError("unterminated mapping".to_owned())),
                    };
                    let key = match key {
                        Value::Seq(_) | Value::Map(_) => {
                            return Err(LoadError("a mapping key is not a scalar".to_owned()));
                        }
                        // An undefined key is the empty one
                        other => other.text().unwrap_or_default(),
                    };
                    let value = match self.events.next() {
                        Some(event) => self.node(event)?,
                        None => return Err(LoadError("missing mapping value".to_owned())),
                    };
                    if entries.iter().any(|(name, _)| *name == key) {
                        return Err(LoadError(format!("Duplicate key '{key}'")));
                    }
                    entries.push((key, value));
                }
                (Value::map(entries), anchor)
            }
            other => return Err(LoadError(format!("unexpected YAML event {other:?}"))),
        };
        if anchor > 0 {
            self.anchors.insert(anchor, value.clone());
        }
        Ok(value)
    }
}

/// Types a scalar: plain scalars as [`resolve_plain`] does, quoted and
/// block scalars as text, and the core tags as asked (`!!str` is text,
/// `!!bool` and `!!null` resolve as plain scalars). Other tags are ignored.
fn scalar(text: &str, style: TScalarStyle, tag: Option<&Tag>) -> Value {
    let core = tag.and_then(|tag| {
        let suffix = if tag.handle == "!!" || tag.handle == "tag:yaml.org,2002:" {
            Some(tag.suffix.as_str())
        } else {
            tag.suffix.strip_prefix("tag:yaml.org,2002:")
        };
        suffix.map(str::to_owned)
    });
    match core.as_deref() {
        Some("str") => Value::Text(text.to_owned()),
        Some("bool" | "null") => resolve_plain(text),
        _ if style == TScalarStyle::Plain => resolve_plain(text),
        _ => Value::Text(text.to_owned()),
    }
}

pub mod edit;

#[cfg(test)]
mod tests;
