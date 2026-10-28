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
/// The name is not empty and has no space, `"`, `=` or control character.
/// The value has no control character, and spaces only where the kernel
/// keeps them in one argument: a value without quotes may have spaces, which
/// [`Display`](fmt::Display) puts in quotes; a value with quotes, such as
/// `b"c d"`, which the kernel keeps as it is, has them in pairs, does not
/// start with one, and has its spaces between them. So an argument is always
/// one argument: a value cannot add others.
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
    /// character, or the value has a control character or quotes that would
    /// not keep it one argument.
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
    let invalid = |c: &char| c.is_control() || (c.is_whitespace() && *c != ' ');
    if let Some(c) = value.chars().find(invalid) {
        return Err(Reason::InvalidCharacter(c));
    }
    if !value.contains('"') {
        return Ok(());
    }
    // Printed as it is, the kernel must read it back the same: no quote
    // first, which it would remove, and no space outside a pair of quotes.
    let mut in_quote = false;
    for c in value.chars() {
        match c {
            '"' => in_quote = !in_quote,
            ' ' if !in_quote => return Err(Reason::InvalidCharacter(' ')),
            _ => {}
        }
    }
    if in_quote || value.starts_with('"') {
        return Err(Reason::InvalidCharacter('"'));
    }
    Ok(())
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
            Some(value) if value.contains(' ') && !value.contains('"') => {
                write!(formatter, "{}=\"{value}\"", self.name)
            }
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

    /// The arguments of `text` that are arguments, and the text of those
    /// that are not, for a command line that is used even when some of it
    /// cannot be: one written by a user, or the kernel's own. The kernel
    /// splits `text` the same way; the arguments left out are those
    /// [`KernelArg::new`] refuses.
    pub fn lenient(text: &str) -> (KernelCommandLine, Vec<String>) {
        let mut args = Vec::new();
        let mut refused = Vec::new();
        for (name, value, raw) in split(text) {
            match KernelArg::new(name, value) {
                Ok(arg) => args.push(arg),
                Err(_) => refused.push(raw.to_owned()),
            }
        }
        (KernelCommandLine(args), refused)
    }

    /// Whether it has no argument.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Adds `arg` at the end.
    pub fn push(&mut self, arg: KernelArg) {
        self.0.push(arg);
    }

    /// The command line without the arguments named one of `names`.
    #[must_use]
    pub fn without(&self, names: &[&str]) -> KernelCommandLine {
        let kept = self.0.iter().filter(|arg| !names.contains(&arg.name()));
        KernelCommandLine(kept.cloned().collect())
    }

    /// The arguments one per line, each followed by a newline: the form of
    /// the command line files of the state directory, which parses back the
    /// same, as a newline separates arguments as a space does.
    pub fn one_per_line(&self) -> String {
        self.0.iter().fold(String::new(), |mut text, arg| {
            text.push_str(&arg.to_string());
            text.push('\n');
            text
        })
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

impl FromIterator<KernelArg> for KernelCommandLine {
    fn from_iter<I: IntoIterator<Item = KernelArg>>(args: I) -> KernelCommandLine {
        KernelCommandLine(args.into_iter().collect())
    }
}

impl Extend<KernelArg> for KernelCommandLine {
    fn extend<I: IntoIterator<Item = KernelArg>>(&mut self, args: I) {
        self.0.extend(args);
    }
}

/// The arguments of `text` as the kernel splits it: each one's name, its
/// value, and its text.
fn split(text: &str) -> Vec<(&str, Option<&str>, &str)> {
    let mut rest = text.trim_start_matches(|c: char| c.is_ascii() && is_space(c as u8));
    let mut args = Vec::new();
    while !rest.is_empty() {
        let (name, value, next) = next_arg(rest);
        let raw = rest
            .get(..rest.len().saturating_sub(next.len()))
            .unwrap_or(rest)
            .trim_end_matches(|c: char| c.is_ascii() && is_space(c as u8));
        args.push((name, value, raw));
        rest = next;
    }
    args
}

impl From<Vec<KernelArg>> for KernelCommandLine {
    fn from(args: Vec<KernelArg>) -> KernelCommandLine {
        KernelCommandLine(args)
    }
}

impl FromStr for KernelCommandLine {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<KernelCommandLine, ParseError> {
        let error = |cause: ParseError| ParseError::new(COMMAND_LINE, text, cause.reason().clone());
        split(text)
            .into_iter()
            .map(|(name, value, _)| KernelArg::new(name, value).map_err(error))
            .collect()
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
