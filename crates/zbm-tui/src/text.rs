//! The text of the menu. Every string a user reads comes from a Project
//! Fluent catalogue, `locales/<language>/menu.ftl`, by message id, never
//! from a literal in the code, so that a translation is a new catalogue and
//! not a change of the code (ROADMAP.md #18). `cargo xtask lint` checks
//! that the menu's code has no text literal, that every id it uses is in
//! the English catalogue, and that every message there is used.
//!
//! [`text!`](crate::text::text) formats a message with its arguments:
//!
//! ```text
//! text!("pool-did-not-import.title", pool = name)
//! ```
//!
//! and [`id!`](crate::text::id) names one to format later, as the key map
//! does for what a key does. An id `message.attribute` names an attribute:
//! the catalogue gives a title its own form (`.title`), as title case is a
//! rule of English. Plurals, numbers and ages are formatted by the
//! catalogue too; logs and the journal stay in English and do not use it.
//!
//! The language is per thread ([`set_language`]), English unless set. The
//! pseudo-language [`Language::Pseudo`] is English with every letter
//! accented and every text a third longer, which the snapshot tests draw so
//! that text that would not fit, or that is not in the catalogue, shows.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

/// The English catalogue.
pub const ENGLISH: &str = include_str!("../locales/en/menu.ftl");

/// A language of the menu.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Language {
    /// English, the catalogue of `locales/en`.
    #[default]
    English,
    /// English accented and lengthened by a third, for the tests.
    Pseudo,
}

impl Language {
    /// The language named `name`: `en` or `pseudo`.
    pub fn from_name(name: &str) -> Option<Language> {
        match name {
            "en" => Some(Language::English),
            "pseudo" => Some(Language::Pseudo),
            _ => None,
        }
    }
}

/// The id of a message of the catalogue, or of one of its attributes
/// (`message.attribute`). Made with [`id!`](crate::text::id), whose literal
/// `cargo xtask lint` checks against the catalogue.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Id(&'static str);

impl Id {
    /// The id `id`; use [`id!`](crate::text::id) instead.
    #[doc(hidden)]
    pub const fn new(id: &'static str) -> Id {
        Id(id)
    }

    /// The id as written in the catalogue.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// The message `id` of the catalogue, formatted with the arguments
/// `name = value`.
macro_rules! text {
    ($id:literal) => {
        $crate::text::message($crate::text::Id::new($id), &[])
    };
    ($id:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::text::message(
            $crate::text::Id::new($id),
            &[$((stringify!($name), $crate::text::Arg::from($value))),+],
        )
    };
}

/// The id of a message of the catalogue, to format later with
/// [`message`] or [`get`].
macro_rules! id {
    ($id:literal) => {
        $crate::text::Id::new($id)
    };
}

pub(crate) use {id, text};

/// A value for a placeable of a message: text or a number.
pub type Arg<'a> = FluentValue<'a>;

thread_local! {
    static LANGUAGE: Cell<Language> = const { Cell::new(Language::English) };
    static BUNDLES: RefCell<Vec<(Language, FluentBundle<FluentResource>)>> =
        const { RefCell::new(Vec::new()) };
}

/// Makes `language` the language of this thread's text, and returns the
/// one it was.
pub fn set_language(language: Language) -> Language {
    LANGUAGE.with(|current| current.replace(language))
}

/// The language of this thread's text.
pub fn language() -> Language {
    LANGUAGE.with(Cell::get)
}

/// The catalogue of `language`, parsed.
fn bundle(language: Language) -> FluentBundle<FluentResource> {
    let english: LanguageIdentifier = "en".parse().unwrap_or_default();
    let mut bundle = FluentBundle::new(vec![english]);
    // No Unicode isolation marks around placeables: terminals would count
    // them as characters.
    bundle.set_use_isolating(false);
    if language == Language::Pseudo {
        bundle.set_transform(Some(pseudo));
    }
    let resource =
        FluentResource::try_new(ENGLISH.to_owned()).unwrap_or_else(|(partial, _)| partial);
    // The lint and the tests check that the catalogue parses without
    // errors and has no message twice.
    let _ = bundle.add_resource(resource);
    bundle
}

/// The message `id` with `args`, in this thread's language; the id itself
/// when the catalogue has no such message, which `cargo xtask lint`
/// prevents.
pub fn message(id: Id, args: &[(&str, Arg<'_>)]) -> String {
    match id.as_str().split_once('.') {
        Some((name, attribute)) => formatted(name, Some(attribute), args),
        None => formatted(id.as_str(), None, args),
    }
}

/// The message `id` without arguments.
pub fn get(id: Id) -> String {
    message(id, &[])
}

/// The title of the message `id`: its attribute `.title`.
pub fn title(id: Id) -> String {
    formatted(id.as_str(), Some("title"), &[])
}

/// The message `name`, or its `attribute`, with `args`, in this thread's
/// language.
fn formatted(name: &str, attribute: Option<&str>, args: &[(&str, Arg<'_>)]) -> String {
    let language = language();
    BUNDLES.with(|bundles| {
        let mut bundles = bundles.borrow_mut();
        if !bundles.iter().any(|(built, _)| *built == language) {
            bundles.push((language, bundle(language)));
        }
        bundles
            .iter()
            .find(|(built, _)| *built == language)
            .and_then(|(_, bundle)| format(bundle, name, attribute, args))
            .unwrap_or_else(|| match attribute {
                Some(attribute) => format!("{name}.{attribute}"),
                None => name.to_owned(),
            })
    })
}

/// The message `name`, or its `attribute`, formatted from `bundle`.
fn format(
    bundle: &FluentBundle<FluentResource>,
    name: &str,
    attribute: Option<&str>,
    args: &[(&str, Arg<'_>)],
) -> Option<String> {
    let message = bundle.get_message(name)?;
    let pattern = match attribute {
        Some(attribute) => message.get_attribute(attribute)?.value(),
        None => message.value()?,
    };
    let mut fluent_args = FluentArgs::new();
    for (name, value) in args {
        fluent_args.set(*name, value.clone());
    }
    let mut errors = Vec::new();
    let text = bundle.format_pattern(pattern, Some(&fluent_args), &mut errors);
    Some(text.into_owned())
}

/// The decimal number written `number` (`1.5`), as a placeable value
/// that keeps its digits after the point.
pub fn decimal(number: &str) -> Arg<'static> {
    number
        .parse::<fluent_bundle::types::FluentNumber>()
        .map_or_else(|_| Arg::from(number.to_owned()), Arg::from)
}

/// The pseudo-language of `text`: its letters accented as the Latin-1
/// letters the console fonts have, and every third letter doubled, so that
/// a text with `n` letters grows by `n / 3`, rounded up.
fn pseudo(text: &str) -> Cow<'_, str> {
    let mut out = String::with_capacity(text.len() * 2);
    let mut letters = 0_usize;
    for c in text.chars() {
        let shown = accented(c);
        out.push(shown);
        if c.is_alphabetic() {
            if letters % 3 == 0 {
                out.push(shown);
            }
            letters += 1;
        }
    }
    Cow::Owned(out)
}

/// `c` with an accent, where Latin-1 has one.
fn accented(c: char) -> char {
    match c {
        'a' => 'á',
        'e' => 'é',
        'i' => 'í',
        'o' => 'ó',
        'u' => 'ú',
        'y' => 'ý',
        'c' => 'ç',
        'n' => 'ñ',
        'A' => 'Á',
        'E' => 'É',
        'I' => 'Í',
        'O' => 'Ó',
        'U' => 'Ú',
        'Y' => 'Ý',
        'C' => 'Ç',
        'N' => 'Ñ',
        other => other,
    }
}

#[cfg(test)]
mod tests;
