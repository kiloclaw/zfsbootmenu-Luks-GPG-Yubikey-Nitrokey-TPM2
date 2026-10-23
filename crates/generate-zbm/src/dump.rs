//! Writing the configuration back, as YAML::PP's `dump_file` does when
//! `--enable` or `--disable` changes `Global.ManageImages`.
//!
//! The Perl program loads the whole file, changes one key and dumps the result,
//! so the file is rewritten in YAML::PP's style: block collections, keys
//! sorted, sequences not indented under their key, anchors for shared
//! collections, and each scalar quoted only when it must be. This module
//! reproduces YAML::PP 0.38's representer (core schema, `boolean` objects) and
//! emitter for the block style it uses.

mod emitter;
mod style;

use std::collections::HashMap;

use crate::config::{Value, format_g15};
use emitter::Emitter;
use style::Style;

/// Dumps a document as YAML::PP does without a `---` header.
pub fn dump(document: &Value) -> String {
    let mut dumper = Dumper::default();
    dumper.count_references(document);
    dumper.emitter.document_start();
    dumper.node(document);
    dumper.emitter.document_end();
    dumper.emitter.output
}

/// The address of a collection, which identifies it when it is shared.
fn address(value: &Value) -> Option<usize> {
    match value {
        Value::Seq(seq) => Some(std::rc::Rc::as_ptr(seq).cast::<()>() as usize),
        Value::Map(map) => Some(std::rc::Rc::as_ptr(map).cast::<()>() as usize),
        _ => None,
    }
}

#[derive(Default)]
struct Dumper {
    emitter: Emitter,
    seen: HashMap<usize, usize>,
    anchors: HashMap<usize, usize>,
    anchor_count: usize,
}

impl Dumper {
    /// Counts how often each collection is reached, as `_check_references` does; a
    /// collection reached twice gets an anchor.
    fn count_references(&mut self, value: &Value) {
        let Some(address) = address(value) else {
            return;
        };
        let seen = self.seen.entry(address).or_insert(0);
        *seen += 1;
        if *seen > 1 {
            return;
        }
        match value {
            Value::Seq(seq) => seq
                .borrow()
                .iter()
                .for_each(|item| self.count_references(item)),
            Value::Map(map) => map
                .borrow()
                .iter()
                .for_each(|(_, item)| self.count_references(item)),
            _ => {}
        }
    }

    fn node(&mut self, value: &Value) {
        let mut anchor = None;
        if let Some(address) = address(value)
            && self.seen.get(&address).copied().unwrap_or(0) > 1
        {
            if let Some(number) = self.anchors.get(&address) {
                self.emitter.alias(*number);
                return;
            }
            self.anchor_count += 1;
            self.anchors.insert(address, self.anchor_count);
            anchor = Some(self.anchor_count);
        }
        match value {
            Value::Seq(seq) => {
                self.emitter.sequence_start(anchor);
                for item in seq.borrow().iter() {
                    self.node(item);
                }
                self.emitter.sequence_end();
            }
            Value::Map(map) => {
                self.emitter.mapping_start(anchor);
                let map = map.borrow();
                let mut entries: Vec<&(String, Value)> = map.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                for (key, item) in entries {
                    self.string(key);
                    self.node(item);
                }
                self.emitter.mapping_end();
            }
            Value::Null => self.emitter.scalar("null", Some(Style::Plain)),
            Value::Bool(value) => self
                .emitter
                .scalar(if *value { "true" } else { "false" }, Some(Style::Plain)),
            Value::Int(number) => self.emitter.scalar(&number.to_string(), Some(Style::Plain)),
            Value::Float(number) => match represent_float(*number) {
                Some(text) => self.emitter.scalar(&text, Some(Style::Plain)),
                None => self.string(&format_g15(*number)),
            },
            Value::Str(text) => self.string(text),
        }
    }

    /// A string: quoted when the core schema would read it as something else,
    /// otherwise styled by the emitter.
    fn string(&mut self, text: &str) {
        let style = reads_as_other_type(text).then_some(Style::Single);
        self.emitter.scalar(text, style);
    }
}

/// The strings YAML::PP quotes because, unquoted, they would be read as null, a
/// boolean or a special number.
const SPECIAL_WORDS: &[&str] = &[
    "", "true", "TRUE", "True", "false", "FALSE", "False", "null", "NULL", "Null", "~", ".inf",
    ".Inf", ".INF", "+.inf", "+.Inf", "+.INF", "-.inf", "-.Inf", "-.INF", ".nan", ".NaN", ".NAN",
];

/// Whether the core schema would read `text` as something other than a string:
/// a special word, or a match of its integer, float, octal or hex patterns. The
/// patterns end with Perl's `$`, which also matches before a final newline.
fn reads_as_other_type(text: &str) -> bool {
    if SPECIAL_WORDS.contains(&text) {
        return true;
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    is_core_int(body) || is_core_float(body) || is_radix(body, "0o", 8) || is_radix(body, "0x", 16)
}

fn is_core_int(text: &str) -> bool {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_radix(text: &str, prefix: &str, radix: u32) -> bool {
    text.strip_prefix(prefix)
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|c| c.is_digit(radix)))
}

/// `^[+-]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][+-]?[0-9]+)?$`
fn is_core_float(text: &str) -> bool {
    let bytes = text.strip_prefix(['+', '-']).unwrap_or(text).as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut index = digits(0);
    if index == 0 {
        if bytes.first() != Some(&b'.') || digits(1) == 0 {
            return false;
        }
        index = 1 + digits(1);
    } else if bytes.get(index) == Some(&b'.') {
        index += 1 + digits(index + 1);
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        let mut exponent = index + 1;
        if matches!(bytes.get(exponent), Some(b'+' | b'-')) {
            exponent += 1;
        }
        let count = digits(exponent);
        if count == 0 {
            return false;
        }
        index = exponent + count;
    }
    index == bytes.len()
}

/// -2^63, the smallest Perl integer.
const IV_MIN: f64 = i64::MIN as f64;

/// 2^64, just above the largest Perl integer.
const UV_LIMIT: f64 = u64::MAX as f64;

/// YAML::PP's `represent_float`: the text of a number written plainly, or
/// `None` when YAML::PP leaves the number to be written as a quoted string.
///
/// Perl compares `0.0 + $n` with `$n` as strings. For an integral number that
/// fits Perl's integer types the sum is an integer and prints with all its
/// digits, so a number that prints in exponent form, such as `1e+15`, does not
/// compare equal and ends up quoted. An integral number that prints without a
/// `.` gets `.0` added.
fn represent_float(number: f64) -> Option<String> {
    if number.is_nan() {
        return Some(".nan".to_owned());
    }
    if number.is_infinite() {
        return Some(if number > 0.0 { ".inf" } else { "-.inf" }.to_owned());
    }
    let text = format_g15(number);
    let fits_integer = number.fract() == 0.0 && (IV_MIN..=UV_LIMIT).contains(&number);
    let sum = if fits_integer {
        integer_digits(number)
    } else {
        text.clone()
    };
    if sum != text {
        return None;
    }
    // Perl's `int`: an integer while the number is above -2^63 and below 2^64, the
    // number itself beyond.
    let int = if number > IV_MIN && number < UV_LIMIT {
        integer_digits(number.trunc())
    } else {
        format_g15(number.trunc())
    };
    if int == text && !text.contains('.') {
        Some(format!("{text}.0"))
    } else {
        Some(text)
    }
}

/// The digits of an integral number that fits 64 bits, as Perl prints an
/// integer; 2^64 itself saturates, as Perl's conversion does.
fn integer_digits(number: f64) -> String {
    if number >= UV_LIMIT {
        return u64::MAX.to_string();
    }
    // The number is integral and in range, so the conversion is exact.
    (number as i128).to_string()
}

#[cfg(test)]
mod tests;
