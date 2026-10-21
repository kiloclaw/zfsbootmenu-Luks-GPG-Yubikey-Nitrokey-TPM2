//! The `generate-zbm` configuration file, read as Perl's YAML::PP reads it.
//!
//! `generate-zbm` loads `config.yaml` with YAML::PP's core schema and booleans,
//! then tests the values with Perl's rules: `has_value` (defined and not
//! empty), truthiness, and `int`. A value's Perl meaning depends on how
//! YAML::PP typed it: `Versions: 3` is the number 3, `Version: 3.10` is the
//! number 3.1 (so it prints as `3.1`), `Enabled: false` is a false boolean that
//! prints as `0`, and an empty value is undefined. [`Value`] keeps those types,
//! and its methods apply Perl's rules to them.
//!
//! Sequences and mappings are shared, as Perl references are: an alias to an
//! anchored mapping is the same mapping, so a change made through one name is
//! seen through the other, and writing the configuration back marks the sharing
//! with anchors.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle};

/// A sequence, shared like a Perl array reference.
pub type Seq = Rc<RefCell<Vec<Value>>>;

/// A mapping in file order, shared like a Perl hash reference.
pub type Map = Rc<RefCell<Vec<(String, Value)>>>;

/// A configuration value, typed as YAML::PP's core schema types it. Cloning a
/// sequence or a mapping shares it, as copying a Perl reference does.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// An undefined value: `null`, `~` or nothing.
    Null,
    /// A boolean (`true`, `False` and so on).
    Bool(bool),
    /// An integer that fits Perl's integer types.
    Int(i128),
    /// A floating-point number, including integers too large for Perl's integer
    /// types.
    Float(f64),
    /// A string.
    Str(String),
    /// A sequence.
    Seq(Seq),
    /// A mapping, in file order.
    Map(Map),
}

/// An error that ends `generate-zbm` the way a Perl `die` outside an `eval`
/// does: with the message on standard error and exit status 255.
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

    /// Whether the value is defined (Perl's `defined`).
    pub fn is_defined(&self) -> bool {
        !matches!(self, Value::Null)
    }

    /// The value as Perl prints it, or `None` when it is undefined.
    pub fn perl_string(&self) -> Option<String> {
        Some(match self {
            Value::Null => return None,
            Value::Bool(true) => "1".to_owned(),
            Value::Bool(false) => "0".to_owned(),
            Value::Int(number) => number.to_string(),
            Value::Float(number) => format_g15(*number),
            Value::Str(text) => text.clone(),
            // References print as their type and address, which differs from run to run. No
            // configuration value of `generate-zbm` is meant to be printed this way.
            Value::Seq(seq) => format!("ARRAY(0x{:x})", address(seq)),
            Value::Map(map) => format!("HASH(0x{:x})", address(map)),
        })
    }

    /// Perl's `has_value` of `generate-zbm`: defined and not empty when printed. A
    /// false boolean prints as `0`, so it has a value.
    pub fn has_value(&self) -> bool {
        self.perl_string().is_some_and(|text| !text.is_empty())
    }

    /// Whether the value is true in Perl: everything but undefined, the empty
    /// string, `"0"`, the number 0 and a false boolean.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(value) => *value,
            Value::Int(number) => *number != 0,
            Value::Float(number) => *number != 0.0,
            Value::Str(text) => !text.is_empty() && text != "0",
            Value::Seq(_) | Value::Map(_) => true,
        }
    }

    /// Perl's `int` of the value: the integer part of its number, where a string
    /// counts as the number at its start (`"3abc"` is 3, `"abc"` is 0).
    pub fn perl_int(&self) -> i64 {
        match self {
            Value::Null | Value::Seq(_) | Value::Map(_) => 0,
            Value::Bool(value) => i64::from(*value),
            Value::Int(number) => {
                i64::try_from(*number).unwrap_or(if *number < 0 { i64::MIN } else { i64::MAX })
            }
            Value::Float(number) => float_to_int(*number),
            Value::Str(text) => float_to_int(numeric_prefix(text)),
        }
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

    /// `$config{section}{key}` in Perl: the value, or `None` when the section or
    /// the key is missing. A section that is defined but not a mapping makes Perl
    /// die.
    pub fn lookup(&self, section: &str, key: &str) -> Result<Option<Value>, Fatal> {
        match self.get(section) {
            None | Some(Value::Null) => Ok(None),
            Some(map @ Value::Map(_)) => Ok(map.get(key).filter(Value::is_defined)),
            Some(other) => Err(not_a_hash(section, &other)),
        }
    }

    /// Sets `$config{section}{key}`, creating the section when it is missing or
    /// undefined, as Perl's autovivification does. A shared section changes
    /// wherever it is used.
    pub fn set(&self, section: &str, key: &str, value: Value) -> Result<(), Fatal> {
        let Value::Map(top) = self else {
            return Err(Fatal("Not a HASH reference".to_owned()));
        };
        let section_map = {
            let mut entries = top.borrow_mut();
            let index = match entries.iter().position(|(name, _)| name == section) {
                Some(index) => index,
                None => {
                    entries.push((section.to_owned(), Value::Null));
                    entries.len() - 1
                }
            };
            let slot = &mut entries[index].1;
            if matches!(slot, Value::Null) {
                *slot = Value::map(Vec::new());
            }
            match slot {
                Value::Map(map) => Rc::clone(map),
                other => return Err(not_a_hash(section, other)),
            }
        };
        let mut fields = section_map.borrow_mut();
        match fields.iter_mut().find(|(name, _)| name == key) {
            Some((_, old)) => *old = value,
            None => fields.push((key.to_owned(), value)),
        }
        Ok(())
    }
}

/// The address of a shared value, which Perl prints for references.
fn address<T>(shared: &Rc<T>) -> usize {
    Rc::as_ptr(shared).cast::<()>() as usize
}

fn not_a_hash(section: &str, value: &Value) -> Fatal {
    match value {
        Value::Seq(_) | Value::Bool(_) => {
            Fatal(format!("Not a HASH reference (section {section})"))
        }
        other => Fatal(format!(
            "Can't use string (\"{}\") as a HASH ref while \"strict refs\" in use \
             (section {section})",
            other.perl_string().unwrap_or_default()
        )),
    }
}

/// Converts a float to an integer as Perl's `int` does: toward zero, saturating
/// at the integer range.
fn float_to_int(number: f64) -> i64 {
    if number.is_nan() {
        0
    } else {
        // `as` truncates toward zero and saturates, which is what `int` does for
        // numbers in range.
        number.trunc() as i64
    }
}

/// The number at the start of a string, as Perl reads a string in numeric
/// context: optional white space and sign, digits with an optional fraction,
/// and an optional exponent. Anything else gives 0.
pub(crate) fn numeric_prefix(text: &str) -> f64 {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let bytes = text.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    let digits_start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let mut mantissa_digits = end - digits_start;
    if bytes.get(end) == Some(&b'.') {
        let fraction_start = end + 1;
        let mut fraction_end = fraction_start;
        while bytes.get(fraction_end).is_some_and(u8::is_ascii_digit) {
            fraction_end += 1;
        }
        mantissa_digits += fraction_end - fraction_start;
        if mantissa_digits > 0 {
            end = fraction_end;
        }
    }
    if mantissa_digits == 0 {
        return 0.0;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exponent_end = end + 1;
        if matches!(bytes.get(exponent_end), Some(b'+' | b'-')) {
            exponent_end += 1;
        }
        let exponent_digits_start = exponent_end;
        while bytes.get(exponent_end).is_some_and(u8::is_ascii_digit) {
            exponent_end += 1;
        }
        if exponent_end > exponent_digits_start {
            end = exponent_end;
        }
    }
    text[..end].parse().unwrap_or(0.0)
}

/// Formats a number as Perl prints a floating-point value, which is C's
/// `%.15g`: 15 significant digits, without trailing zeros, in exponent form for
/// very large and very small numbers.
pub fn format_g15(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_owned();
    }
    if number.is_infinite() {
        return if number > 0.0 { "Inf" } else { "-Inf" }.to_owned();
    }
    if number == 0.0 {
        // Perl prints a negative zero as `0`.
        return "0".to_owned();
    }
    let scientific = format!("{number:.14e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("exponent notation has an e");
    let exponent: i32 = exponent.parse().expect("the exponent is a number");
    if !(-4..15).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{}e{sign}{:02}",
            strip_trailing_zeros(mantissa),
            exponent.abs()
        )
    } else {
        let decimals = usize::try_from(14 - exponent).expect("the exponent is below 15");
        strip_trailing_zeros(&format!("{number:.decimals$}")).to_owned()
    }
}

fn strip_trailing_zeros(text: &str) -> &str {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    }
}

/// Types a plain scalar with YAML::PP's core schema: null, boolean, decimal,
/// octal and hex integers, floats, infinities and NaN, and otherwise a string.
pub fn resolve_plain(text: &str) -> Value {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => {
            return Value::Float(f64::INFINITY);
        }
        "-.inf" | "-.Inf" | "-.INF" => return Value::Float(f64::NEG_INFINITY),
        ".nan" | ".NaN" | ".NAN" => return Value::Float(f64::NAN),
        _ => {}
    }
    if is_decimal_int(text) {
        return decimal_value(text);
    }
    if let Some(digits) = text.strip_prefix("0o") {
        if !digits.is_empty() && digits.bytes().all(|byte| (b'0'..=b'7').contains(&byte)) {
            return radix_value(digits, 8);
        }
    }
    if let Some(digits) = text.strip_prefix("0x") {
        if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return radix_value(digits, 16);
        }
    }
    if is_core_float(text) {
        return Value::Float(text.parse().unwrap_or(0.0));
    }
    Value::Str(text.to_owned())
}

/// `^[+-]?[0-9]+$`
fn is_decimal_int(text: &str) -> bool {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// `0 + "<decimal>"` in Perl: an integer while it fits 64 bits (signed, or
/// unsigned when positive), a float beyond.
fn decimal_value(text: &str) -> Value {
    match text.parse::<i128>() {
        Ok(number) if (i128::from(i64::MIN)..=i128::from(u64::MAX)).contains(&number) => {
            Value::Int(number)
        }
        _ => Value::Float(text.parse().unwrap_or(0.0)),
    }
}

/// Perl's `oct` and `hex` of digits: an integer while it fits 64 bits, a float
/// beyond.
fn radix_value(digits: &str, radix: u32) -> Value {
    match u64::from_str_radix(digits, radix) {
        Ok(number) => Value::Int(i128::from(number)),
        Err(_) => {
            let float = digits.bytes().fold(0.0_f64, |value, byte| {
                value * f64::from(radix) + f64::from(char::from(byte).to_digit(radix).unwrap_or(0))
            });
            Value::Float(float)
        }
    }
}

/// `^[+-]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][+-]?[0-9]+)?$`
fn is_core_float(text: &str) -> bool {
    let rest = text.strip_prefix(['+', '-']).unwrap_or(text);
    let bytes = rest.as_bytes();
    let mut index = 0;
    let integer_digits = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    index += integer_digits;
    if integer_digits == 0 {
        // `\.[0-9]+`
        if bytes.get(index) != Some(&b'.') {
            return false;
        }
        index += 1;
        let fraction = bytes[index..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if fraction == 0 {
            return false;
        }
        index += fraction;
    } else if bytes.get(index) == Some(&b'.') {
        // `[0-9]+(\.[0-9]*)?`
        index += 1;
        index += bytes[index..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let exponent = bytes[index..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if exponent == 0 {
            return false;
        }
        index += exponent;
    }
    index == bytes.len()
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
                        // Perl uses the printed form of a key; an undefined key becomes the empty
                        // string.
                        other => other.perl_string().unwrap_or_default(),
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

/// Types a scalar: plain scalars by the core schema, quoted and block scalars
/// as strings, and core tags as requested. Other tags are ignored, as YAML::PP
/// ignores unknown local tags.
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
        Some("str") => Value::Str(text.to_owned()),
        Some("int" | "float" | "bool" | "null") => match resolve_plain(text) {
            Value::Str(_) => Value::Str(text.to_owned()),
            typed => typed,
        },
        _ if style == TScalarStyle::Plain => resolve_plain(text),
        _ => Value::Str(text.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testutil::legacy::perl;
    use zbm_testutil::{Rng, TempDir, have_program};

    #[test]
    fn plain_scalars_follow_the_core_schema() {
        assert_eq!(resolve_plain(""), Value::Null);
        assert_eq!(resolve_plain("~"), Value::Null);
        assert_eq!(resolve_plain("True"), Value::Bool(true));
        assert_eq!(resolve_plain("FALSE"), Value::Bool(false));
        assert_eq!(resolve_plain("007"), Value::Int(7));
        assert_eq!(resolve_plain("-0"), Value::Int(0));
        assert_eq!(resolve_plain("0o17"), Value::Int(15));
        assert_eq!(resolve_plain("0x10"), Value::Int(16));
        assert_eq!(resolve_plain("3.10"), Value::Float(3.1));
        assert_eq!(resolve_plain("1e3"), Value::Float(1000.0));
        assert_eq!(resolve_plain(".5"), Value::Float(0.5));
        assert_eq!(resolve_plain("5."), Value::Float(5.0));
        assert_eq!(resolve_plain("7.3.*"), Value::Str("7.3.*".to_owned()));
        assert_eq!(resolve_plain("1e"), Value::Str("1e".to_owned()));
        assert_eq!(resolve_plain("0x"), Value::Str("0x".to_owned()));
    }

    #[test]
    fn numbers_print_like_perl() {
        assert_eq!(format_g15(3.1), "3.1");
        assert_eq!(format_g15(1000.0), "1000");
        assert_eq!(format_g15(1e20), "1e+20");
        assert_eq!(format_g15(1e-6), "1e-06");
        assert_eq!(format_g15(1.5e-7), "1.5e-07");
        assert_eq!(format_g15(0.0001), "0.0001");
        assert_eq!(format_g15(0.5), "0.5");
        assert_eq!(format_g15(123456789012345.0), "123456789012345");
        assert_eq!(format_g15(1234567890123456.0), "1.23456789012346e+15");
        assert_eq!(format_g15(f64::INFINITY), "Inf");
        assert_eq!(format_g15(f64::NAN), "NaN");
    }

    #[test]
    fn perl_rules() {
        assert!(Value::Bool(false).has_value());
        assert!(!Value::Bool(false).truthy());
        assert!(!Value::Str(String::new()).has_value());
        assert!(Value::Str("0.0".to_owned()).truthy());
        assert!(!Value::Str("0".to_owned()).truthy());
        assert_eq!(Value::Str("3abc".to_owned()).perl_int(), 3);
        assert_eq!(Value::Str("1e2".to_owned()).perl_int(), 100);
        assert_eq!(Value::Str("-4.7".to_owned()).perl_int(), -4);
        assert_eq!(Value::Str("0x10".to_owned()).perl_int(), 0);
        assert_eq!(Value::Str(" 12".to_owned()).perl_int(), 12);
        assert_eq!(Value::Bool(true).perl_int(), 1);
        assert_eq!(Value::Float(3.9).perl_int(), 3);
    }

    #[test]
    fn documents() {
        assert!(load("").is_err());
        assert!(load("- a\n").is_err());
        assert!(load("just text\n").is_err());
        assert!(load("a: 1\na: 2\n").is_err());
        let config = load("a: &x 1\nb: *x\n---\nc: 2\n").unwrap();
        assert_eq!(config.get("b"), Some(Value::Int(1)));
        assert_eq!(config.get("c"), None);
        let config =
            load("a: !!str 3\nb: !!int \"4\"\nc: !foo bar\nd: 'q'\ne: >-\n  folded\n  text\n")
                .unwrap();
        assert_eq!(config.get("a"), Some(Value::Str("3".to_owned())));
        assert_eq!(config.get("b"), Some(Value::Int(4)));
        assert_eq!(config.get("c"), Some(Value::Str("bar".to_owned())));
        assert_eq!(config.get("d"), Some(Value::Str("q".to_owned())));
        assert_eq!(config.get("e"), Some(Value::Str("folded text".to_owned())));
    }

    #[test]
    fn sections() {
        let config = load("Global:\n  ManageImages: true\nEFI: text\n").unwrap();
        assert_eq!(
            config.lookup("Global", "ManageImages").unwrap(),
            Some(Value::Bool(true))
        );
        assert_eq!(config.lookup("Kernel", "Path").unwrap(), None);
        assert!(config.lookup("EFI", "Enabled").is_err());
        config
            .set("Components", "Enabled", Value::Bool(false))
            .unwrap();
        assert_eq!(
            config.lookup("Components", "Enabled").unwrap(),
            Some(Value::Bool(false))
        );
    }

    #[test]
    fn aliases_share_collections() {
        let config =
            load("Other: &g\n  BootMountPoint: /boot\nGlobal: *g\nList: &l [1]\nCopy: *l\n")
                .unwrap();
        config
            .set("Global", "ManageImages", Value::Bool(true))
            .unwrap();
        assert_eq!(
            config.lookup("Other", "ManageImages").unwrap(),
            Some(Value::Bool(true))
        );
        assert!(load("a: &x [1, *x]\n").is_err());
        let config = load("Global:\n").unwrap();
        config
            .set("Global", "ManageImages", Value::Bool(false))
            .unwrap();
        assert_eq!(
            config.lookup("Global", "ManageImages").unwrap(),
            Some(Value::Bool(false))
        );
    }

    /// For each key of a generated mapping, what Perl sees: defined, printed form,
    /// truth and `int`.
    fn perl_view(yaml: &str) -> Option<String> {
        let dir = TempDir::new("generate-zbm-config");
        let file = dir.write("config.yaml", yaml);
        let program = r#"
            use YAML::PP; use boolean; no warnings;
            my $y = eval { YAML::PP->new(boolean => "boolean")->load_file($ARGV[0]) };
            exit 3 unless defined $y and ref $y eq "HASH";
            for my $k (sort keys %$y) {
                my $v = $y->{$k};
                if (ref $v eq "ARRAY" or ref $v eq "HASH") { print "$k|ref\n"; next; }
                my $i = int($v); $i = "big" if $i != $i or abs($i) >= 1e15;
                printf "%s|%s|%s|%s|%s\n", $k, (defined $v ? 1 : 0),
                    (defined $v ? "$v" : ""), ($v ? 1 : 0), $i;
            }
        "#;
        let output = perl(program, &[&file.to_string_lossy()], b"");
        output.status.success().then(|| output.stdout_text())
    }

    fn rust_view(yaml: &str) -> Option<String> {
        let Ok(Value::Map(map)) = load(yaml) else {
            return None;
        };
        let mut entries = map.borrow().clone();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Some(
            entries
                .iter()
                .map(|(key, value)| match value {
                    Value::Seq(_) | Value::Map(_) => format!("{key}|ref\n"),
                    value => {
                        // Integers beyond 15 digits and NaN are not meaningful configuration
                        // values; Perl prints them in other forms, so only their kind is compared.
                        let int = value.perl_int();
                        let nan = matches!(value, Value::Float(number) if number.is_nan());
                        let int = if nan || int.unsigned_abs() >= 1_000_000_000_000_000 {
                            "big".to_owned()
                        } else {
                            int.to_string()
                        };
                        format!(
                            "{key}|{}|{}|{}|{int}\n",
                            u8::from(value.is_defined()),
                            value.perl_string().unwrap_or_default(),
                            u8::from(value.truthy()),
                        )
                    }
                })
                .collect(),
        )
    }

    #[test]
    fn values_match_yaml_pp() {
        if !have_program("perl") {
            eprintln!("skipped: perl is missing");
            return;
        }
        let scalars = [
            "",
            "~",
            "null",
            "NULL",
            "true",
            "True",
            "false",
            "FALSE",
            "yes",
            "no",
            "0",
            "1",
            "3",
            "007",
            "-0",
            "+5",
            "0o17",
            "0x1F",
            "0x",
            "3.10",
            "1e3",
            ".5",
            "5.",
            "1.0",
            "-4.7",
            "1e20",
            "1e-6",
            ".inf",
            "-.Inf",
            ".nan",
            "7.3.*",
            "6.1",
            "abc",
            "3abc",
            "'0'",
            "\"\"",
            "'007'",
            "\"1e3\"",
            "!!str 12",
            "!!int \"9\"",
            "[a, b]",
            "{x: 1}",
            "/boot/efi",
            "quiet loglevel=4",
            "12345678901234567890",
            "99999999999999999999",
        ];
        let mut rng = Rng::from_env(8);
        for _ in 0..60 {
            let count = 1 + rng.below(6);
            let yaml: String = (0..count)
                .map(|index| format!("k{index}: {}\n", rng.choose(&scalars)))
                .collect();
            assert_eq!(rust_view(&yaml), perl_view(&yaml), "yaml {yaml:?}");
        }
    }
}
