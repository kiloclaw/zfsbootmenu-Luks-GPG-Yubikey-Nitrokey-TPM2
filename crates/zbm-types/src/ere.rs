//! POSIX extended regular expressions as Bash's `[[ text =~ pattern ]]`
//! reads them in the C locale, as the legacy code matched them: glibc's
//! `regcomp` with `REG_EXTENDED`. [`translate`] turns one into the syntax
//! of the `regex` crate, which matches it in the process; no shell runs.
//!
//! What glibc reads, and its translation:
//!
//! - Bytes, not characters: the C locale has one byte per character. Each
//!   literal becomes `\xHH`, and the `regex` crate runs without Unicode.
//! - `.` is any byte but NUL, newline included.
//! - `^` and `$` are anchors wherever they are, so `a^b` matches nothing.
//! - `*`, `+`, `?` and the intervals `{m}`, `{m,}`, `{m,n}`, `{,n}` and
//!   `{,}` repeat what is before them, as often as they follow each other
//!   (`a**`, `a{2}{3}`); with nothing before them, after `(`, `|` or an
//!   anchor, the pattern is not valid. Counts go up to 32767.
//! - An unmatched `)` and a `}` are literals; `\` makes any other byte
//!   one.
//! - Brackets: `]` first is a literal, `-` first or last too; ranges are
//!   of byte values; the classes `[:alpha:]` and the others are ASCII, and
//!   `[.c.]` and `[=c=]` name one byte. A backslash is a literal there.
//! - The GNU operators: `\w`, `\W`, `\s`, `\S`, `\b`, `\B`, `\<`, `\>`,
//!   `` \` `` and `\'`.
//!
//! Back-references (`\1` to `\9`) have no translation: the `regex` crate
//! has none, and a pattern with one is refused. So are patterns whose
//! groups and repetitions nest more than [`NEST_MAX`] deep.
//!
//! One quirk of glibc is left out: in a text with a newline, `$` matches
//! before it when more of the pattern follows (`a$.b` matches "a\nb",
//! though `a$` does not), and `^` after it. The texts matched here are
//! kernel paths from one-per-line lists, which hold no newline.

use std::fmt::Write as _;

/// Why a pattern is not valid, as a clause: "it ...".
pub type Invalid = &'static str;

/// The largest count of an interval: glibc's `RE_DUP_MAX`.
const DUP_MAX: u32 = 0x7fff;

/// The longest name between `[:` and `:]` (or `[.` `.]`, `[=` `=]`) glibc
/// reads, terminator included: its `BRACKET_NAME_BUF_SIZE`.
const NAME_MAX: usize = 32;

/// How deep groups and repetitions may nest: the `regex` crate's default.
pub const NEST_MAX: usize = 250;

/// The pattern `pattern` in the syntax of the `regex` crate, for a byte
/// regular expression without Unicode.
///
/// # Errors
///
/// Why `pattern` is not valid, or cannot be translated.
pub fn translate(pattern: &[u8]) -> Result<String, Invalid> {
    let mut parser = Parser {
        bytes: pattern,
        at: 0,
        groups: 0,
        nesting: 0,
    };
    parser.alternation()
}

/// A parser over the bytes of a pattern.
struct Parser<'a> {
    /// The pattern.
    bytes: &'a [u8],
    /// The next byte.
    at: usize,
    /// The groups open around the next byte.
    groups: usize,
    /// How deep the output nests there.
    nesting: usize,
}

/// What a backslash makes of the byte after it.
enum Escape {
    /// Something to match, which can be repeated.
    Atom(String),
    /// A position, which cannot.
    Anchor(&'static str),
}

/// An element of a bracket expression.
#[derive(Clone, Copy)]
enum Element {
    /// A byte: by itself, or named by `[.c.]`.
    Byte(u8),
    /// A class, such as `[:alpha:]`, which cannot be an end of a range.
    Class(fn(u8) -> bool),
    /// A byte named by `[=c=]`, which cannot be one either.
    Equivalent(u8),
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.at.saturating_add(ahead)).copied()
    }

    /// The byte at the front, which is there.
    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.at = self.at.saturating_add(1);
        Some(byte)
    }

    /// Branches separated by `|`.
    fn alternation(&mut self) -> Result<String, Invalid> {
        let mut out = self.branch()?;
        while self.peek() == Some(b'|') {
            self.at = self.at.saturating_add(1);
            out.push('|');
            out.push_str(&self.branch()?);
        }
        Ok(out)
    }

    /// Expressions one after another, up to a `|`, the `)` of an open
    /// group, or the end.
    fn branch(&mut self) -> Result<String, Invalid> {
        let mut out = String::new();
        loop {
            match self.peek() {
                None | Some(b'|') => return Ok(out),
                Some(b')') if self.groups > 0 => return Ok(out),
                Some(_) => out.push_str(&self.expression()?),
            }
        }
    }

    /// An anchor, or an atom with its repetitions.
    fn expression(&mut self) -> Result<String, Invalid> {
        let Some(byte) = self.next() else {
            return Ok(String::new());
        };
        let atom = match byte {
            b'^' => return Ok("^".to_owned()),
            b'$' => return Ok("$".to_owned()),
            b'*' | b'+' | b'?' | b'{' => return Err("it has a repetition with nothing to repeat"),
            b'(' => self.group()?,
            b'.' => r"[^\x00]".to_owned(),
            b'[' => self.bracket()?,
            b'\\' => match self.escape()? {
                Escape::Atom(atom) => atom,
                Escape::Anchor(anchor) => return Ok(anchor.to_owned()),
            },
            other => literal(other),
        };
        self.repetitions(atom)
    }

    /// The rest of a group, after its `(`.
    fn group(&mut self) -> Result<String, Invalid> {
        self.deeper()?;
        self.groups = self.groups.saturating_add(1);
        let inner = self.alternation()?;
        if self.next() != Some(b')') {
            return Err("it has a '(' that is not closed");
        }
        self.groups = self.groups.saturating_sub(1);
        self.nesting = self.nesting.saturating_sub(1);
        Ok(format!("(?:{inner})"))
    }

    /// One level deeper in the output.
    fn deeper(&mut self) -> Result<(), Invalid> {
        self.nesting = self.nesting.saturating_add(1);
        if self.nesting > NEST_MAX {
            return Err("its groups and repetitions nest too deeply");
        }
        Ok(())
    }

    /// `atom` with the repetitions that follow it.
    fn repetitions(&mut self, mut atom: String) -> Result<String, Invalid> {
        let outer = self.nesting;
        while let Some(byte @ (b'*' | b'+' | b'?' | b'{')) = self.peek() {
            self.at = self.at.saturating_add(1);
            let operator = match byte {
                b'{' => self.interval()?,
                other => char::from(other).to_string(),
            };
            self.deeper()?;
            atom = format!("(?:{atom}){operator}");
        }
        self.nesting = outer;
        Ok(atom)
    }

    /// The rest of an interval, after its `{`, as the `regex` crate
    /// writes it.
    fn interval(&mut self) -> Result<String, Invalid> {
        const BAD: Invalid = "it has an interval that is not valid";
        let low = self.number();
        let high = match self.next() {
            Some(b'}') => Some(low.ok_or(BAD)?),
            Some(b',') => {
                let high = self.number();
                if self.next() != Some(b'}') {
                    return Err(BAD);
                }
                high
            }
            _ => return Err(BAD),
        };
        let low = low.unwrap_or(0);
        if high.unwrap_or(low).max(low) > DUP_MAX {
            return Err("it has an interval above 32767");
        }
        match high {
            Some(high) if high < low => Err(BAD),
            Some(high) if high == low => Ok(format!("{{{low}}}")),
            Some(high) => Ok(format!("{{{low},{high}}}")),
            None => Ok(format!("{{{low},}}")),
        }
    }

    /// The decimal number at the front, if there is one; counts above the
    /// largest stop growing there.
    fn number(&mut self) -> Option<u32> {
        let mut value: Option<u32> = None;
        while let Some(digit @ b'0'..=b'9') = self.peek() {
            self.at = self.at.saturating_add(1);
            let digit = u32::from(digit - b'0');
            let grown = value.unwrap_or(0).saturating_mul(10).saturating_add(digit);
            value = Some(grown.min(DUP_MAX.saturating_add(1)));
        }
        value
    }

    /// The rest of an escape, after its `\`.
    fn escape(&mut self) -> Result<Escape, Invalid> {
        let Some(byte) = self.next() else {
            return Err("it ends with '\\'");
        };
        Ok(match byte {
            b'1'..=b'9' => return Err("it has a back-reference, which is not supported"),
            b'w' => Escape::Atom(set(is_word, false)),
            b'W' => Escape::Atom(set(is_word, true)),
            b's' => Escape::Atom(set(is_space, false)),
            b'S' => Escape::Atom(set(is_space, true)),
            b'b' => Escape::Anchor(r"\b"),
            b'B' => Escape::Anchor(r"\B"),
            b'<' => Escape::Anchor(r"\b{start}"),
            b'>' => Escape::Anchor(r"\b{end}"),
            b'`' => Escape::Anchor(r"\A"),
            b'\'' => Escape::Anchor(r"\z"),
            other => Escape::Atom(literal(other)),
        })
    }

    /// The rest of a bracket expression, after its `[`.
    fn bracket(&mut self) -> Result<String, Invalid> {
        const NOT_CLOSED: Invalid = "it has a '[' that is not closed";
        let negated = self.peek() == Some(b'^');
        if negated {
            self.at = self.at.saturating_add(1);
        }
        let mut bytes = [false; 256];
        let mut first = true;
        loop {
            match self.peek() {
                None => return Err(NOT_CLOSED),
                Some(b']') if !first => {
                    self.at = self.at.saturating_add(1);
                    break;
                }
                Some(_) => {}
            }
            let start = self.element(first)?;
            first = false;
            let range = matches!(start, Element::Byte(_))
                && self.peek() == Some(b'-')
                && self.peek_at(1).is_some_and(|after| after != b']');
            if !range {
                add(&mut bytes, start);
                continue;
            }
            self.at = self.at.saturating_add(1);
            let (Element::Byte(low), Element::Byte(high)) = (start, self.element(true)?) else {
                return Err("it has a range that ends with a class");
            };
            if low > high {
                return Err("it has a range that ends before it starts");
            }
            for byte in low..=high {
                hold(&mut bytes, byte);
            }
        }
        Ok(byte_class(|byte| has_byte(&bytes, byte) != negated))
    }

    /// An element of a bracket expression. A `-` is one when it is
    /// `first`, or last before the `]`.
    fn element(&mut self, first: bool) -> Result<Element, Invalid> {
        let byte = self.next().ok_or("it has a '[' that is not closed")?;
        let opener = self.peek().filter(|_| byte == b'[');
        match opener {
            Some(delimiter @ (b':' | b'.' | b'=')) => {
                self.at = self.at.saturating_add(1);
                let name = self.name(delimiter)?;
                match delimiter {
                    b':' => class(name).map(Element::Class),
                    _ => match name {
                        [byte] if delimiter == b'.' => Ok(Element::Byte(*byte)),
                        [byte] => Ok(Element::Equivalent(*byte)),
                        _ => Err("it names a collating element that is not one character"),
                    },
                }
            }
            _ if byte == b'-' && !first && self.peek() != Some(b']') => {
                Err("it has a '-' that is neither a range nor last")
            }
            _ => Ok(Element::Byte(byte)),
        }
    }

    /// The name of `[:name:]`, `[.name.]` or `[=name=]`, after its opener,
    /// up to `delimiter` and `]`.
    fn name(&mut self, delimiter: u8) -> Result<&[u8], Invalid> {
        const NOT_CLOSED: Invalid = "it has a '[' that is not closed";
        let start = self.at;
        loop {
            let byte = self.next().ok_or(NOT_CLOSED)?;
            if self.peek().is_none() {
                return Err(NOT_CLOSED);
            }
            if byte == delimiter && self.peek() == Some(b']') {
                let end = self.at.saturating_sub(1);
                self.at = self.at.saturating_add(1);
                return Ok(self.bytes.get(start..end).unwrap_or_default());
            }
            if self.at.saturating_sub(start) >= NAME_MAX {
                return Err(NOT_CLOSED);
            }
        }
    }
}

/// Adds what `element` matches to `bytes`.
fn add(bytes: &mut [bool; 256], element: Element) {
    match element {
        Element::Byte(byte) | Element::Equivalent(byte) => hold(bytes, byte),
        Element::Class(class) => {
            for (value, slot) in (0..=u8::MAX).zip(bytes.iter_mut()) {
                *slot |= class(value);
            }
        }
    }
}

/// Adds `byte` to `bytes`.
fn hold(bytes: &mut [bool; 256], byte: u8) {
    if let Some(slot) = bytes.get_mut(usize::from(byte)) {
        *slot = true;
    }
}

/// Whether `bytes` holds `byte`.
fn has_byte(bytes: &[bool; 256], byte: u8) -> bool {
    bytes.get(usize::from(byte)).copied().unwrap_or(false)
}

/// The class named `name`, as the C locale has it.
fn class(name: &[u8]) -> Result<fn(u8) -> bool, Invalid> {
    Ok(match name {
        b"alpha" => |byte: u8| byte.is_ascii_alphabetic(),
        b"upper" => |byte: u8| byte.is_ascii_uppercase(),
        b"lower" => |byte: u8| byte.is_ascii_lowercase(),
        b"digit" => |byte: u8| byte.is_ascii_digit(),
        b"xdigit" => |byte: u8| byte.is_ascii_hexdigit(),
        b"alnum" => |byte: u8| byte.is_ascii_alphanumeric(),
        b"space" => is_space,
        b"blank" => |byte: u8| matches!(byte, b' ' | b'\t'),
        b"punct" => |byte: u8| byte.is_ascii_punctuation(),
        b"graph" => |byte: u8| byte.is_ascii_graphic(),
        b"print" => |byte: u8| byte.is_ascii_graphic() || byte == b' ',
        b"cntrl" => |byte: u8| byte.is_ascii_control(),
        _ => return Err("it names a character class that does not exist"),
    })
}

/// Whether `byte` is a word character: `\w`.
fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether `byte` is white space: `\s` and `[:space:]`.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t'..=b'\r')
}

/// The bytes `class` holds, or the others when `negated`.
fn set(class: fn(u8) -> bool, negated: bool) -> String {
    byte_class(|byte| class(byte) != negated)
}

/// A byte as a literal of the `regex` crate.
fn literal(byte: u8) -> String {
    format!(r"\x{byte:02X}")
}

/// A class of the `regex` crate with the bytes `holds` keeps, as ranges.
fn byte_class(holds: impl Fn(u8) -> bool) -> String {
    let mut out = String::from("[");
    let mut start: Option<u8> = None;
    for byte in 0..=u8::MAX {
        match (holds(byte), start) {
            (true, None) => start = Some(byte),
            (false, Some(low)) => {
                push_range(&mut out, low, byte.saturating_sub(1));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(low) = start {
        push_range(&mut out, low, u8::MAX);
    }
    if out.len() == 1 {
        // No byte: a class that holds none.
        return r"[\x00&&\x01]".to_owned();
    }
    out.push(']');
    out
}

fn push_range(out: &mut String, low: u8, high: u8) {
    let _ = if low == high {
        write!(out, r"\x{low:02X}")
    } else {
        write!(out, r"\x{low:02X}-\x{high:02X}")
    };
}

#[cfg(test)]
mod tests;
