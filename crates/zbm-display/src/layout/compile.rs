//! Layouts compiled when the image is built, from the build host's XKB data
//! (`xkeyboard-config`) and compose rules (`libX11`), into files the image
//! carries: one self-contained keymap per layout, and the compose rules of
//! dead keys.

use std::fmt::Write as _;
use std::path::Path;

use kbvm::xkb::Context;
use kbvm::xkb::diagnostic::{Diagnostic, DiagnosticKind};
use kbvm::xkb::rmlvo::Group;

use super::LayoutError;

/// Where the XKB data usually is.
pub const XKB: &str = "/usr/share/X11/xkb";

/// The compose rules of the `en_US.UTF-8` locale, whose dead keys every
/// layout shares.
pub const COMPOSE: &str = "/usr/share/X11/locale/en_US.UTF-8/Compose";

/// Whether `part` names a layout or a variant: letters, digits, `_` and `-`.
fn is_word(part: &str) -> bool {
    !part.is_empty()
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The layout and the variant of `name`: `de`, or `de(nodeadkeys)`.
///
/// # Errors
///
/// When `name` is neither.
pub fn split(name: &str) -> Result<(&str, &str), LayoutError> {
    let (layout, variant) = match name.split_once('(') {
        Some((layout, rest)) => (layout, rest.strip_suffix(')').filter(|v| is_word(v))),
        None => (name, Some("")),
    };
    match variant {
        Some(variant) if is_word(layout) => Ok((layout, variant)),
        _ => Err(LayoutError::Invalid(format!(
            "{name}: not a layout name, such as de or de(nodeadkeys)"
        ))),
    }
}

/// The keymap of layout `name` (`de`, `de(nodeadkeys)`) on a PC keyboard,
/// from the XKB data in `xkb`, as one self-contained file.
///
/// # Errors
///
/// When `name` is not a layout name or the data has no such layout.
pub fn keymap(xkb: &Path, name: &str) -> Result<String, LayoutError> {
    let (layout, variant) = split(name)?;
    let mut builder = Context::builder();
    builder.enable_default_includes(false);
    builder.enable_environment(false);
    builder.append_path(xkb);
    let context = builder.build();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let keymap = context.keymap_from_names(
        &mut diagnostics,
        Some("evdev"),
        Some("pc105"),
        Some(&[Group { layout, variant }]),
        None,
    );
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind() == DiagnosticKind::FileNotFound)
    {
        return Err(LayoutError::Unknown(name.to_owned()));
    }
    Ok(keymap.format().to_string())
}

/// `text` as a compose file's string: in quotes, with `"` and `\` escaped.
fn quoted(text: &str) -> String {
    let mut quoted = String::from("\"");
    for c in text.chars() {
        if matches!(c, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

/// The rules of `compose`, a compose file, that start with a dead key or
/// the compose key (`Multi_key`), in the same format: what the console
/// needs of the rules of a locale.
///
/// # Errors
///
/// When `compose` does not parse.
pub fn dead_keys(compose: &[u8]) -> Result<String, LayoutError> {
    let context = super::sealed_context();
    let mut builder = context.compose_table_builder();
    builder.buffer(compose);
    let mut ignored: Vec<Diagnostic> = Vec::new();
    let table = builder
        .build(&mut ignored)
        .ok_or_else(|| LayoutError::Invalid("the compose rules do not parse".to_owned()))?;
    let mut rules = String::new();
    let mut iter = table.iter();
    while let Some(rule) = iter.next() {
        let names: Option<Vec<&str>> = rule
            .steps()
            .iter()
            .map(|step| step.keysym().name())
            .collect();
        let (Some(names), Some(string)) = (names, rule.string()) else {
            continue;
        };
        let is_wanted = names
            .first()
            .is_some_and(|first| first.starts_with("dead_") || *first == "Multi_key");
        if !is_wanted {
            continue;
        }
        for name in names {
            let _ = write!(rules, "<{name}> ");
        }
        let _ = writeln!(rules, ": {}", quoted(string));
    }
    Ok(rules)
}
