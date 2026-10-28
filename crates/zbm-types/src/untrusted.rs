//! Text from outside ZFSBootMenu, kept from reaching a terminal raw.
//!
//! A pool's names, a boot environment's files and what programs print are
//! chosen by whoever made the pool or the device (attackers A2 and A3 of
//! `docs/dev/threat-model.md`). Written to a terminal as they are, their
//! control characters move the cursor, change colours, set the window
//! title, or answer for the user. [`Untrusted`] holds such text and has no
//! `Display`: it is shown through [`Printable`], which writes each control
//! character as an escape that a terminal does not act on, or it is parsed
//! into a domain type, which checks it.
//!
//! ```
//! use zbm_types::{Printable, Untrusted};
//!
//! let name = Untrusted::new("zroot/\u{1b}]0;owned\u{7}ROOT".to_owned());
//! assert_eq!(name.printable().to_string(), "zroot/\\x1b]0;owned\\x07ROOT");
//! assert_eq!(Printable("plain").to_string(), "plain");
//! ```

use std::fmt::{self, Write as _};
use std::str::FromStr;

/// Text that came from outside ZFSBootMenu: shown only as [`Printable`],
/// used only once parsed.
#[derive(Clone, Default, Eq, Hash, PartialEq)]
pub struct Untrusted<T>(T);

impl<T> Untrusted<T> {
    /// Marks `value` as untrusted.
    pub fn new(value: T) -> Untrusted<T> {
        Untrusted(value)
    }

    /// The value itself, for code that checks it, such as a parser, or that
    /// passes it on as data, never to a terminal.
    pub fn into_inner(self) -> T {
        self.0
    }

    /// The value, borrowed, under the same rule as [`Untrusted::into_inner`].
    pub fn get(&self) -> &T {
        &self.0
    }
}

impl<T: AsRef<str>> Untrusted<T> {
    /// The text, to show.
    pub fn printable(&self) -> Printable<'_> {
        Printable(self.0.as_ref())
    }

    /// The text parsed as a `U`, which checks it.
    ///
    /// # Errors
    ///
    /// The error of `U`'s parser.
    pub fn parse<U: FromStr>(&self) -> Result<U, U::Err> {
        self.0.as_ref().parse()
    }
}

impl<T: AsRef<str>> fmt::Debug for Untrusted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Untrusted(\"{}\")", self.printable())
    }
}

/// Text shown so that no character in it acts on a terminal: C0 controls
/// (tab and newline included), DEL, C1 controls, and the characters that
/// reorder text (bidi overrides and isolates) are written as `\xNN` or
/// `\u{NNNN}`. Everything else is written as it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Printable<'a>(pub &'a str);

impl fmt::Display for Printable<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut rest = self.0;
        while let Some(at) = rest.find(must_escape) {
            let (plain, tail) = rest.split_at(at);
            f.write_str(plain)?;
            let mut chars = tail.chars();
            if let Some(c) = chars.next() {
                escape(f, c)?;
            }
            rest = chars.as_str();
        }
        f.write_str(rest)
    }
}

impl Printable<'_> {
    /// Whether the text is written as it is, with nothing escaped.
    pub fn is_plain(&self) -> bool {
        !self.0.contains(must_escape)
    }
}

/// Whether `c` acts on a terminal, or reorders the text around it.
pub fn must_escape(c: char) -> bool {
    c.is_control() || is_bidi_control(c)
}

/// The characters that override the direction of the text after them:
/// LRE, RLE, PDF, LRO, RLO (U+202A to U+202E), LRI, RLI, FSI and PDI
/// (U+2066 to U+2069), and LRM, RLM (U+200E, U+200F) and ALM (U+061C).
fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}'
    )
}

/// Writes `c` as an escape: `\xNN` below U+0100, else `\u{NNNN}`.
fn escape(f: &mut fmt::Formatter<'_>, c: char) -> fmt::Result {
    let code = u32::from(c);
    if code < 0x100 {
        write!(f, "\\x{code:02x}")
    } else {
        f.write_str("\\u{")?;
        write!(f, "{code:04x}")?;
        f.write_char('}')
    }
}

#[cfg(test)]
mod tests;
