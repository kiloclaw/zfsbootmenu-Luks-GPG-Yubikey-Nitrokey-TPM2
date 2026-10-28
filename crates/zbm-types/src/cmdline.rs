//! Kernel command lines, split into arguments as the kernel splits them
//! (`next_arg` in Linux's `lib/cmdline.c`).

use std::fmt;
use std::str::FromStr;

use crate::error::{ParseError, Reason};

/// What [`ParseError::what`] says for a kernel argument.
const ARGUMENT: &str = "kernel argument";

/// What [`ParseError::what`] says for a kernel command line.
const COMMAND_LINE: &str = "kernel command line";

/// Whether the kernel's `isspace` holds for `byte`.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// One argument of a kernel command line: a name such as `quiet`, or a
/// name and a value such as `root=zfs:zroot/ROOT/void`.
///
/// The name is not empty and has no space, `"`, `=` or control character;
/// the value has no `"` or control character but may have spaces, which
/// [`Display`](fmt::Display) puts in quotes, as the kernel reads them. So an
/// argument is always one argument: a value cannot add others.
///
/// ```
/// use zbm_types::KernelArg;
///
/// let root = KernelArg::new("root", Some("zfs:tank/my ROOT"))?;
/// assert_eq!(root.to_string(), "root=\"zfs:tank/my ROOT\"");
/// assert_eq!(root.to_string().parse::<KernelArg>()?, root);
/// assert!("quiet init=/bin/sh".parse::<KernelArg>().is_err());
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KernelArg {
    /// The part before the first `=`.
    name: String,
    /// The part after it, if the argument has one.
    value: Option<String>,
}

impl KernelArg {
    /// The argument `name`, with `value` if it has one.
    ///
    /// # Errors
    ///
    /// When the name is empty or has a space, `"`, `=` or control
    /// character, or the value has a `"` or control character.
    pub fn new(name: &str, value: Option<&str>) -> Result<KernelArg, ParseError> {
        let text = match value {
            Some(value) => format!("{name}={value}"),
            None => name.to_owned(),
        };
        let error = |reason| ParseError::new(ARGUMENT, &text, reason);
        check_name(name).map_err(error)?;
        if let Some(value) = value {
            check_value(value).map_err(error)?;
        }
        Ok(KernelArg {
            name: name.to_owned(),
            value: value.map(str::to_owned),
        })
    }

    /// The name: the part before the first `=`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The value: the part after the first `=`, if there is one.
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }
}

/// Why `name` cannot be the name of an argument, if it cannot.
fn check_name(name: &str) -> Result<(), Reason> {
    if name.is_empty() {
        return Err(Reason::Empty);
    }
    let invalid = |c: &char| c.is_whitespace() || c.is_control() || matches!(c, '"' | '=');
    match name.chars().find(invalid) {
        Some(c) => Err(Reason::InvalidCharacter(c)),
        None => Ok(()),
    }
}

/// Why `value` cannot be the value of an argument, if it cannot.
fn check_value(value: &str) -> Result<(), Reason> {
    let invalid = |c: &char| c.is_control() || *c == '"' || (c.is_whitespace() && *c != ' ');
    match value.chars().find(invalid) {
        Some(c) => Err(Reason::InvalidCharacter(c)),
        None => Ok(()),
    }
}

impl FromStr for KernelArg {
    type Err = ParseError;

    /// One argument, as the kernel reads it: `name`, `name=value`,
    /// `name="value"` or `"name=value"`.
    fn from_str(text: &str) -> Result<KernelArg, ParseError> {
        let error = |reason| ParseError::new(ARGUMENT, text, reason);
        if let Some(space) = text
            .chars()
            .next()
            .filter(|c| c.is_ascii() && is_space(*c as u8))
        {
            return Err(error(Reason::InvalidCharacter(space)));
        }
        let (name, value, rest) = next_arg(text);
        if !rest.is_empty() {
            return Err(error(Reason::NotOneArgument));
        }
        KernelArg::new(name, value).map_err(|cause| error(cause.reason().clone()))
    }
}

impl fmt::Display for KernelArg {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value {
            None => formatter.write_str(&self.name),
            Some(value) if value.contains(' ') => write!(formatter, "{}=\"{value}\"", self.name),
            Some(value) => write!(formatter, "{}={value}", self.name),
        }
    }
}

/// The first argument of `text`, which does not start with a space, split
/// as Linux's `next_arg` splits it: its name, its value if it has an `=`
/// (but not as its first character), and the text after the spaces that
/// follow it. Quotes group spaces into the argument; one quote around the
/// whole argument, or around its value, is removed.
fn next_arg(text: &str) -> (&str, Option<&str>, &str) {
    let quoted = text.starts_with('"');
    let args = if quoted {
        text.get(1..).unwrap_or("")
    } else {
        text
    };
    let mut in_quote = quoted;
    let mut equals = 0;
    let mut end = args.len();
    for (at, byte) in args.bytes().enumerate() {
        if is_space(byte) && !in_quote {
            end = at;
            break;
        }
        if equals == 0 && byte == b'=' {
            equals = at;
        }
        if byte == b'"' {
            in_quote = !in_quote;
        }
    }
    let rest = args
        .get(end..)
        .unwrap_or("")
        .trim_start_matches(|c: char| c.is_ascii() && is_space(c as u8));
    let arg = args.get(..end).unwrap_or("");
    // A closing quote is dropped once, as the kernel overwrites it once.
    let closes = arg.ends_with('"');
    if equals == 0 {
        let name = if quoted && closes {
            strip_last(arg)
        } else {
            arg
        };
        return (name, None, rest);
    }
    let name = arg.get(..equals).unwrap_or("");
    let value = arg.get(equals + 1..).unwrap_or("");
    let value = match value.strip_prefix('"') {
        Some(inner) if closes => strip_last(inner),
        Some(inner) => inner,
        None if quoted && closes => strip_last(value),
        None => value,
    };
    (name, Some(value), rest)
}

/// `text` without its last character, a `"`; empty when that is all of it.
fn strip_last(text: &str) -> &str {
    text.strip_suffix('"').unwrap_or("")
}

/// A kernel command line: arguments separated by spaces, such as
/// `root=zfs:zroot/ROOT/void quiet loglevel=4`.
///
/// It parses as the kernel splits a command line, and prints each argument
/// so that it parses back the same.
///
/// ```
/// use zbm_types::KernelCommandLine;
///
/// let text = "quiet  init_on_free=1 dyndbg=\"file a.c +p\"";
/// let line: KernelCommandLine = text.parse()?;
/// assert_eq!(line.value("init_on_free"), Some("1"));
/// assert_eq!(line.value("dyndbg"), Some("file a.c +p"));
/// assert_eq!(line.to_string(), "quiet init_on_free=1 dyndbg=\"file a.c +p\"");
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct KernelCommandLine(Vec<KernelArg>);

impl KernelCommandLine {
    /// The arguments, in order.
    pub fn args(&self) -> &[KernelArg] {
        &self.0
    }

    /// The value of the last argument named `name`, which the kernel and
    /// most programs take; `None` without one, or when it has no value.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .rev()
            .find(|arg| arg.name == name)
            .and_then(KernelArg::value)
    }
}

impl From<Vec<KernelArg>> for KernelCommandLine {
    fn from(args: Vec<KernelArg>) -> KernelCommandLine {
        KernelCommandLine(args)
    }
}

impl FromStr for KernelCommandLine {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<KernelCommandLine, ParseError> {
        let error = |reason| ParseError::new(COMMAND_LINE, text, reason);
        let mut rest = text.trim_start_matches(|c: char| c.is_ascii() && is_space(c as u8));
        let mut args = Vec::new();
        while !rest.is_empty() {
            let (name, value, next) = next_arg(rest);
            let arg = KernelArg::new(name, value).map_err(|cause| error(cause.reason().clone()))?;
            args.push(arg);
            rest = next;
        }
        Ok(KernelCommandLine(args))
    }
}

impl fmt::Display for KernelCommandLine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (at, arg) in self.0.iter().enumerate() {
            if at > 0 {
                formatter.write_str(" ")?;
            }
            arg.fmt(formatter)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
