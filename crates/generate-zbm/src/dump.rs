//! Writing the configuration back, as YAML::PP's `dump_file` does when
//! `--enable` or `--disable` changes `Global.ManageImages`.
//!
//! The Perl program loads the whole file, changes one key and dumps the result,
//! so the file is rewritten in YAML::PP's style: block collections, keys
//! sorted, sequences not indented under their key, anchors for shared
//! collections, and each scalar quoted only when it must be. This module
//! reproduces YAML::PP 0.38's representer (core schema, `boolean` objects) and
//! emitter for the block style it uses.

use std::collections::HashMap;

use crate::config::{Value, format_g15};

/// How a scalar is written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Style {
    Plain,
    Single,
    Double,
    Literal,
}

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

/// One level of the emitter's state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Document,
    Mapping,
    MappingValue,
    Sequence,
    ComplexKey,
    ComplexValue,
}

#[derive(Clone, Debug)]
struct Frame {
    kind: Kind,
    index: usize,
    indent: String,
    newline: bool,
    column: bool,
    zero_indent: bool,
}

/// The block-style part of YAML::PP's emitter, with its indentation of two
/// spaces.
#[derive(Default)]
struct Emitter {
    output: String,
    column: usize,
    stack: Vec<Frame>,
    open_ended: bool,
}

const INDENT: &str = "  ";

impl Emitter {
    fn write(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let last_line = text.rsplit('\n').next().unwrap_or_default();
        if last_line.is_empty() {
            self.column = 0;
        } else if text.contains('\n') {
            self.column = last_line.chars().count();
        } else {
            self.column += last_line.chars().count();
        }
        self.output.push_str(text);
    }

    fn last(&mut self) -> &mut Frame {
        self.stack
            .last_mut()
            .expect("events arrive inside a document")
    }

    fn document_start(&mut self) {
        let column = self.column != 0;
        self.stack = vec![Frame {
            kind: Kind::Document,
            index: 0,
            indent: String::new(),
            newline: false,
            column,
            zero_indent: false,
        }];
    }

    fn document_end(&mut self) {
        self.stack.clear();
        if self.open_ended {
            self.write("...\n");
            self.open_ended = false;
        } else {
            self.open_ended = true;
        }
    }

    /// The common start of a block mapping or sequence: the `-`, `?` or `:` that
    /// introduces it, and its anchor. Returns the new level's indentation, whether
    /// its first entry starts on a new line, and whether a sequence is not
    /// indented.
    fn collection_start(&mut self, anchor: Option<usize>, sequence: bool) -> (String, bool, bool) {
        let mut yaml = String::new();
        let last = self
            .stack
            .last_mut()
            .expect("events arrive inside a document");
        let indent = last.indent.clone();
        let mut new_indent = indent.clone();
        let mut newline = false;
        let mut zero_indent = false;
        if last.kind == Kind::Document {
            newline = last.newline;
        } else {
            if last.newline {
                yaml.push('\n');
                last.column = false;
            }
            if last.kind == Kind::MappingValue {
                if sequence {
                    zero_indent = true;
                } else {
                    new_indent.push_str(INDENT);
                }
                newline = true;
            } else {
                new_indent.push_str(INDENT);
                yaml.push_str(if last.column { " " } else { &indent });
                match last.kind {
                    Kind::Sequence => yaml.push('-'),
                    Kind::Mapping => {
                        last.kind = Kind::ComplexKey;
                        zero_indent = sequence;
                        yaml.push('?');
                    }
                    Kind::ComplexValue => {
                        zero_indent = sequence;
                        yaml.push(':');
                    }
                    other => unreachable!("a collection cannot start in {other:?}"),
                }
                last.column = true;
            }
            last.newline = false;
        }
        if let Some(anchor) = anchor {
            yaml.push_str(if last.column { " " } else { &indent });
            yaml.push_str(&format!("&{anchor}"));
            newline = true;
        }
        last.index += 1;
        self.write(&yaml);
        (new_indent, newline, zero_indent)
    }

    fn push(&mut self, kind: Kind, indent: String, newline: bool, zero_indent: bool) {
        let column = self.column != 0;
        self.stack.push(Frame {
            kind,
            index: 0,
            indent,
            newline,
            column,
            zero_indent,
        });
        self.open_ended = false;
    }

    fn mapping_start(&mut self, anchor: Option<usize>) {
        let (indent, newline, _) = self.collection_start(anchor, false);
        self.push(Kind::Mapping, indent, newline, false);
    }

    fn sequence_start(&mut self, anchor: Option<usize>) {
        let (indent, newline, zero_indent) = self.collection_start(anchor, true);
        self.push(Kind::Sequence, indent, newline, zero_indent);
    }

    fn collection_end(&mut self, empty: &str) {
        let frame = self.stack.pop().expect("a collection was started");
        if frame.index == 0 {
            let mut indent = frame.indent;
            if frame.zero_indent {
                indent.push_str(INDENT);
            }
            let lead = if self.column != 0 { " " } else { &indent };
            self.write(&format!("{lead}{empty}\n"));
        }
        let column = self.column != 0;
        let last = self.last();
        last.column = column;
        last.kind = match last.kind {
            Kind::Mapping => Kind::MappingValue,
            Kind::MappingValue | Kind::ComplexValue => Kind::Mapping,
            Kind::ComplexKey => Kind::ComplexValue,
            other => other,
        };
    }

    fn mapping_end(&mut self) {
        self.collection_end("{}");
    }

    fn sequence_end(&mut self) {
        self.collection_end("[]");
    }

    fn alias(&mut self, number: usize) {
        let mut yaml = String::new();
        let last = self
            .stack
            .last_mut()
            .expect("events arrive inside a document");
        if matches!(last.kind, Kind::Mapping | Kind::Sequence) && last.index == 0 && last.newline {
            yaml.push('\n');
            last.column = false;
            last.newline = false;
        }
        yaml.push_str(if last.column { " " } else { &last.indent });
        if last.kind == Kind::Mapping {
            yaml.push_str(&format!("*{number} :"));
            last.kind = Kind::MappingValue;
        } else {
            match last.kind {
                Kind::MappingValue => last.kind = Kind::Mapping,
                Kind::Document => {}
                Kind::ComplexValue => {
                    last.kind = Kind::Mapping;
                    yaml.push_str(": ");
                }
                Kind::ComplexKey => yaml.push_str(": "),
                Kind::Sequence => yaml.push_str("- "),
                Kind::Mapping => unreachable!("handled above"),
            }
            yaml.push_str(&format!("*{number}\n"));
        }
        last.index += 1;
        self.write(&yaml);
        let column = self.column != 0;
        self.last().column = column;
        self.open_ended = false;
    }

    fn scalar(&mut self, value: &str, requested: Option<Style>) {
        let style = best_style(value, requested);
        let indent = self.last().indent.clone();
        let mut open_ended = false;
        let text = match style {
            Style::Plain => value.to_owned(),
            Style::Single => single_quoted(value, &format!("{indent}{INDENT}")),
            Style::Literal => {
                let (text, ended) = literal(value, &format!("{indent}{INDENT}"));
                open_ended = ended;
                text
            }
            Style::Double => double_quoted(value),
        };
        let yaml = self.block_scalar(&text, style, &indent);
        let last = self.last();
        last.index += 1;
        last.newline = false;
        self.write(&yaml);
        let column = self.column != 0;
        self.last().column = column;
        self.open_ended = open_ended;
    }

    /// `_emit_block_scalar`: places a scalar as a key, a value or a sequence entry.
    fn block_scalar(&mut self, value: &str, style: Style, indent: &str) -> String {
        let mut yaml = String::new();
        let mut text = value.to_owned();
        let multiline = style == Style::Literal;
        let last = self
            .stack
            .last_mut()
            .expect("events arrive inside a document");
        if matches!(last.kind, Kind::Mapping | Kind::Sequence) && last.index == 0 && last.newline {
            yaml.push('\n');
            last.column = false;
            last.newline = false;
        }
        if last.kind == Kind::Mapping {
            yaml.push_str(if last.column { " " } else { indent });
            last.kind = Kind::MappingValue;
            if multiline {
                // A multi-line key is written as a complex key.
                yaml.push_str("? ");
                last.kind = Kind::ComplexValue;
            } else {
                text.push(':');
            }
        } else {
            match last.kind {
                Kind::MappingValue => last.kind = Kind::Mapping,
                Kind::Document => {}
                kind => {
                    yaml.push_str(if last.column { " " } else { indent });
                    if kind == Kind::ComplexValue {
                        last.kind = Kind::Mapping;
                        yaml.push(':');
                    } else if kind == Kind::Sequence {
                        yaml.push('-');
                    } else {
                        unreachable!("a scalar cannot be written in {kind:?}");
                    }
                    last.column = true;
                }
            }
            if !text.is_empty() && last.column {
                text.insert(0, ' ');
            }
            if !multiline {
                text.push('\n');
            }
        }
        yaml.push_str(&text);
        yaml
    }
}

/// Characters YAML::PP writes only in double quotes.
fn is_control(c: char) -> bool {
    matches!(
        c,
        '\u{0}'..='\u{8}'
            | '\u{b}'
            | '\u{c}'
            | '\u{e}'..='\u{1f}'
            | '\u{7f}'..='\u{84}'
            | '\u{86}'..='\u{9f}'
            | '\u{fffe}'
            | '\u{ffff}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{85}'
            | '\u{a0}'
    )
}

/// Characters that YAML allows in a plain scalar.
fn is_printable(c: char) -> bool {
    matches!(
        c,
        '\u{20}'..='\u{7e}'
            | '\u{85}'
            | '\u{a0}'..='\u{d7ff}'
            | '\u{e000}'..='\u{fefe}'
            | '\u{ff00}'..='\u{fffd}'
            | '\u{10000}'..='\u{10ffff}'
    )
}

/// `_find_best_scalar_style` for block context.
fn best_style(value: &str, requested: Option<Style>) -> Style {
    let mut style = requested;
    if value.is_empty() {
        style = style.or(Some(Style::Single));
    } else if value.chars().any(is_control) {
        style = Some(Style::Double);
    }
    let mut style = style.unwrap_or(Style::Plain);
    let first = value.chars().next();
    match style {
        Style::Single => {
            if value.contains(" \n")
                || value.contains("\n ")
                || value.starts_with('\n')
                || value.ends_with('\n')
            {
                style = Style::Double;
            }
        }
        Style::Plain if !value.is_empty() => {
            style = if value.contains(['\t', '\r']) || value == "\n" {
                Style::Double
            } else if value.chars().all(|c| c == ' ') {
                Style::Single
            } else if value.chars().all(|c| c == ' ' || c == '\n') {
                Style::Double
            } else if value.contains('\n') {
                Style::Literal
            } else if first.is_some_and(|c| "!&*{}[]|>@`\"'#%, ".contains(c))
                || value.starts_with("---")
                || value.starts_with("...")
                || value.contains(": ")
                || value.contains(" #")
                || value.ends_with([':', ' ', '\t'])
                || !value.chars().all(is_printable)
                || (first.is_some_and(|c| "?-:".contains(c))
                    && value.chars().nth(1).is_none_or(char::is_whitespace))
            {
                Style::Single
            } else {
                Style::Plain
            };
        }
        _ => {}
    }
    if style == Style::Single && requested.is_none() && value.contains('\'') && !value.contains('"')
    {
        style = Style::Double;
    }
    style
}

fn single_quoted(value: &str, continuation: &str) -> String {
    // Runs of line breaks are doubled, as single-quoted scalars fold single breaks.
    let mut folded = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            let mut run = 1;
            while chars.peek() == Some(&'\n') {
                chars.next();
                run += 1;
            }
            folded.push_str(&"\n".repeat(run + 1));
        } else {
            folded.push(c);
        }
    }
    let lines: Vec<String> = folded
        .split('\n')
        .enumerate()
        .map(|(index, line)| {
            if index > 0 && !line.is_empty() {
                format!("{continuation}{line}")
            } else {
                line.to_owned()
            }
        })
        .collect();
    format!("'{}'", lines.join("\n").replace('\'', "''"))
}

/// A literal block scalar and whether it leaves the document open-ended (kept
/// trailing line breaks).
fn literal(value: &str, indent: &str) -> (String, bool) {
    let mut indicators = String::new();
    if value.trim_start_matches('\n').starts_with(' ') {
        indicators.push('2');
    }
    let mut text = value.to_owned();
    let mut open_ended = false;
    if !text.ends_with('\n') {
        indicators.push('-');
        text.push('\n');
    } else if text.ends_with("\n\n") || text == "\n" {
        indicators.push('+');
        open_ended = true;
    }
    let body: String = text
        .split_inclusive('\n')
        .map(|line| {
            if line.starts_with('\n') {
                line.to_owned()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect();
    (format!("|{indicators}\n{body}"), open_ended)
}

fn double_quoted(value: &str) -> String {
    let mut text = String::from('"');
    for c in value.chars() {
        let escaped = match c {
            '\n' => "\\n".to_owned(),
            '\t' => "\\t".to_owned(),
            '\r' => "\\r".to_owned(),
            '\\' => "\\\\".to_owned(),
            '"' => "\\\"".to_owned(),
            '\u{0}' => "\\0".to_owned(),
            '\u{7}' => "\\a".to_owned(),
            '\u{8}' => "\\b".to_owned(),
            '\u{b}' => "\\v".to_owned(),
            '\u{c}' => "\\f".to_owned(),
            '\u{1b}' => "\\e".to_owned(),
            '\u{85}' => "\\N".to_owned(),
            '\u{a0}' => "\\_".to_owned(),
            '\u{2028}' => "\\L".to_owned(),
            '\u{2029}' => "\\P".to_owned(),
            '\u{fffe}' | '\u{ffff}' => format!("\\u{:04x}", u32::from(c)),
            c if is_control(c) => format!("\\x{:02x}", u32::from(c)),
            c => c.to_string(),
        };
        text.push_str(&escaped);
    }
    text.push('"');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load;
    use zbm_testkit::legacy::perl;
    use zbm_testkit::{Rng, TempDir, have_program};

    #[test]
    fn the_default_configuration() {
        let config = load(
            "Global:\n  ManageImages: false\n  BootMountPoint: /boot/efi\n\
             Components:\n  Versions: 3\n  Enabled: true\nEFI:\n  Versions: false\n\
             Kernel:\n  CommandLine: ro quiet loglevel=0\n",
        )
        .unwrap();
        config
            .set("Global", "ManageImages", Value::Bool(true))
            .unwrap();
        assert_eq!(
            dump(&config),
            "Components:\n  Enabled: true\n  Versions: 3\nEFI:\n  Versions: false\n\
             Global:\n  BootMountPoint: /boot/efi\n  ManageImages: true\n\
             Kernel:\n  CommandLine: ro quiet loglevel=0\n"
        );
    }

    #[test]
    fn scalars() {
        let cases = [
            ("", "''"),
            ("a: b", "'a: b'"),
            ("'q", "\"'q\""),
            ("quote'", "quote'"),
            ("123", "'123'"),
            ("x\n", "|\n    x\n"),
            ("a\tb", "\"a\\tb\""),
            ("-a", "-a"),
            ("- ", "'- '"),
        ];
        for (value, expected) in cases {
            let config = Value::map(vec![(
                "k".to_owned(),
                Value::map(vec![("v".to_owned(), Value::Str(value.to_owned()))]),
            )]);
            let written = dump(&config);
            let expected = if expected.starts_with('|') {
                format!("k:\n  v: {expected}")
            } else {
                format!("k:\n  v: {expected}\n")
            };
            assert_eq!(written, expected, "value {value:?}");
        }
        assert_eq!(represent_float(1000.0).as_deref(), Some("1000.0"));
        assert_eq!(represent_float(1e20).as_deref(), Some("1e+20.0"));
        assert_eq!(represent_float(1e15), None);
        assert_eq!(represent_float(3.1).as_deref(), Some("3.1"));
        assert_eq!(represent_float(-0.0).as_deref(), Some("0.0"));
    }

    /// What YAML::PP writes for a configuration after `--enable`.
    fn perl_dump(yaml: &str) -> Option<String> {
        let dir = TempDir::new("generate-zbm-dump");
        let input = dir.write("in.yaml", yaml);
        let output = dir.join("out.yaml");
        let program = r#"
            use strict; use YAML::PP; use boolean; no warnings;
            my $y = eval { YAML::PP->new(boolean => "boolean")->load_file($ARGV[0]) };
            exit 3 unless defined $y and ref $y eq "HASH";
            my %config = %$y;
            $config{Global}{ManageImages} = boolean(1);
            YAML::PP->new(boolean => "boolean", header => 0)->dump_file($ARGV[1], \%config);
        "#;
        let result = perl(
            program,
            &[&input.to_string_lossy(), &output.to_string_lossy()],
            b"",
        );
        result
            .status
            .success()
            .then(|| std::fs::read_to_string(&output).unwrap())
    }

    fn rust_dump(yaml: &str) -> Option<String> {
        let config = load(yaml).ok()?;
        config
            .set("Global", "ManageImages", Value::Bool(true))
            .ok()?;
        Some(dump(&config))
    }

    #[test]
    fn matches_yaml_pp() {
        if !have_program("perl") {
            eprintln!("skipped: perl is missing");
            return;
        }
        let scalars = [
            "~",
            "true",
            "no",
            "0",
            "-12",
            "007",
            "0o17",
            "0x1F",
            "3.10",
            "1e3",
            "1e15",
            "1e20",
            "-0.0",
            "1.5e-7",
            ".inf",
            "-.inf",
            ".nan",
            "99999999999999999999",
            "18446744073709551615",
            "abc",
            "'123'",
            "''",
            "' lead'",
            "'trail '",
            "'a: b'",
            "'#c'",
            "'x #y'",
            "'true'",
            "\"multi\\nline\"",
            "\"two\\n\\n\"",
            "\" sp\\nx\"",
            "\"tab\\there\"",
            "\"quote'\"",
            "'dq\"'",
            "'-'",
            "'- x'",
            "'?'",
            "'@at'",
            "'%p'",
            "'*star'",
            "'[br'",
            "\"ünï\"",
            "\"\\u00a0x\"",
            "\"\\ufeffx\"",
            "\"bell\\a\"",
            "'a\\b'",
            "'1.0'",
            "'0x10'",
            "1_000",
            "Yes",
            "'~'",
            "'--- x'",
            "'...'",
            "'a:'",
            "/boot/efi",
            "ro quiet loglevel=0",
            "[]",
            "{}",
            "[a, b]",
            "{x: 1, y: [2]}",
            "[[p, q], {r: s}]",
        ];
        let keys = ["a", "B", "123", "'key: x'", "''", "Global", "z"];
        let mut rng = Rng::from_env(9);
        for _ in 0..120 {
            let mut yaml = String::new();
            let mut anchored = Vec::new();
            for (index, key) in keys.iter().enumerate() {
                if rng.below(2) == 0 {
                    continue;
                }
                let value = if !anchored.is_empty() && rng.below(5) == 0 {
                    format!("*{}", rng.choose(&anchored))
                } else if rng.below(6) == 0 {
                    anchored.push(format!("n{index}"));
                    format!("&n{index} {{k{index}: {}}}", rng.choose(&scalars))
                } else if rng.below(4) == 0 {
                    format!(
                        "\n  s: {}\n  t: {}",
                        rng.choose(&scalars),
                        rng.choose(&scalars)
                    )
                } else {
                    (*rng.choose(&scalars)).to_owned()
                };
                yaml.push_str(&format!("{key}: {value}\n"));
            }
            assert_eq!(rust_dump(&yaml), perl_dump(&yaml), "yaml {yaml:?}");
        }
    }
}
