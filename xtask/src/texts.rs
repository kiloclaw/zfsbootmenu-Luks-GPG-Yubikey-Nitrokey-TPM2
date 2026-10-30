//! The menu's text (ROADMAP.md, R5): every string a user reads
//! comes from the English catalogue `crates/zbm-tui/locales/en/menu.ftl`.
//! `cargo xtask lint` checks that
//!
//! - the menu's code has no text literal outside test code: a string with a
//!   letter outside the placeholders of a format string, other than the
//!   message id of `text!` or `id!`;
//! - every message id the menu names is in the catalogue;
//! - every message and attribute of the catalogue is used. A `.title` is
//!   used when its message is named, as the toolbar and the help view look
//!   titles up from the ids of what keys do.
//!
//! The files of [`NOT_TEXT`] hold strings that are not text the user reads.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use crate::error::{Error, Result};
use crate::metrics::tree::scan_package;
use crate::workspace::Workspace;

mod scan;

use self::scan::{Found, TextScan, scan};

/// The package of the menu.
pub const PACKAGE: &str = "zbm-tui";

/// The English catalogue, relative to the root.
pub const CATALOGUE: &str = "crates/zbm-tui/locales/en/menu.ftl";

/// The menu's code, whose text comes from the catalogue, relative to the
/// root.
pub const MENU: &str = "crates/zbm-tui/src/menu";

/// Paths below [`MENU`] whose strings are not text the user reads, each with
/// the reason.
pub const NOT_TEXT: &[(&str, &str)] = &[
    (
        "demo",
        "the fake machine: its data stands for what the machine says",
    ),
    ("caps.rs", "terminal names and environment variables"),
    (
        "theme.rs",
        "the glyphs of the ASCII theme, symbols drawn with letters",
    ),
    (
        "keyboard.rs",
        "the name of the console's own keyboard layout",
    ),
    ("display.rs", "device paths, and messages for developers"),
    ("display", "the display without a terminal, for developers"),
    ("terminal.rs", "the key log of the measures, for developers"),
    (
        "cells.rs",
        "the client channel's variable, and errors the menu does not show",
    ),
    (
        "model/naming.rs",
        "the names the legacy menu offers for new environments and snapshots",
    ),
];

/// One message or attribute of the catalogue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// Its line.
    pub line: usize,
    /// Its id: `message` or `message.attribute`.
    pub id: String,
}

/// The messages and attributes of a catalogue, in order. A message starts
/// a line with its id and ` =`; an attribute is an indented line that
/// starts with `.`, its id and ` =`.
pub fn entries(catalogue: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut message = String::new();
    for (index, line) in catalogue.lines().enumerate() {
        let name = |text: &str| -> Option<String> {
            let (name, _) = text.split_once(" =")?;
            let valid = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            valid.then(|| name.to_owned())
        };
        if line.starts_with(|c: char| c.is_ascii_alphabetic()) {
            if let Some(name) = name(line) {
                message.clone_from(&name);
                entries.push(Entry {
                    line: index + 1,
                    id: name,
                });
            }
        } else if let Some(attribute) = line.trim_start().strip_prefix('.')
            && line.starts_with(' ')
            && let Some(attribute) = name(attribute)
        {
            entries.push(Entry {
                line: index + 1,
                id: format!("{message}.{attribute}"),
            });
        }
    }
    entries
}

/// Whether the catalogue entry `entry` is used by the ids `named`.
fn is_used(entry: &str, named: &HashSet<&str>) -> bool {
    if let Some((message, attribute)) = entry.split_once('.') {
        return named.contains(entry) || (attribute == "title" && named.contains(message));
    }
    let prefix = format!("{entry}.");
    named.contains(entry) || named.iter().any(|id| id.starts_with(&prefix))
}

/// Whether `path` is in the menu's code and holds text the user reads.
fn is_text_file(root: &Path, path: &Path) -> bool {
    let Ok(below) = path.strip_prefix(root.join(MENU)) else {
        return false;
    };
    !NOT_TEXT
        .iter()
        .any(|(excluded, _)| below.starts_with(excluded))
}

/// The problems of the scanned files against the catalogue's `entries`.
pub fn problems(
    catalogue: &str,
    entries: &[Entry],
    files: &[(String, bool, TextScan)],
) -> Vec<String> {
    let known: HashSet<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
    let mut problems = Vec::new();
    let mut named = HashSet::new();
    for (path, holds_text, scan) in files {
        if *holds_text {
            problems.extend(scan.literals.iter().map(|Found { line, value }| {
                format!("{path}:{line}: text {value:?} is not from the catalogue")
            }));
        }
        for Found { line, value } in scan.ids.iter().chain(&scan.test_ids) {
            if !known.contains(value.as_str()) {
                problems.push(format!(
                    "{path}:{line}: no message {value:?} in {catalogue}"
                ));
            }
        }
        named.extend(scan.ids.iter().map(|found| found.value.as_str()));
    }
    let mut seen = BTreeSet::new();
    for entry in entries {
        if !seen.insert(entry.id.as_str()) {
            problems.push(format!("{catalogue}:{}: {} again", entry.line, entry.id));
        }
        if !is_used(&entry.id, &named) {
            problems.push(format!(
                "{catalogue}:{}: {} is not used",
                entry.line, entry.id
            ));
        }
    }
    problems
}

/// Checks the menu's text.
///
/// # Errors
///
/// [`Error::Check`] listing every problem; [`Error::Io`], [`Error::Parse`]
/// or [`Error::Format`] when the sources or the catalogue cannot be read.
pub fn check(workspace: &Workspace) -> Result<()> {
    let root = &workspace.root;
    let package = workspace
        .package(PACKAGE)
        .ok_or_else(|| Error::Format(format!("no package {PACKAGE}")))?;
    let catalogue = std::fs::read_to_string(root.join(CATALOGUE))
        .map_err(|error| Error::io(format!("reading {CATALOGUE}"), error))?;
    let mut files = Vec::new();
    for file in scan_package(package, &HashSet::new())? {
        let text = std::fs::read_to_string(&file.path)
            .map_err(|error| Error::io(format!("reading {}", file.path.display()), error))?;
        let parsed = syn::parse_file(&text).map_err(|source| Error::Parse {
            path: file.path.clone(),
            source,
        })?;
        let shown = file.path.strip_prefix(root).unwrap_or(&file.path);
        let holds_text = !file.test_code && is_text_file(root, &file.path);
        files.push((
            shown.display().to_string(),
            holds_text,
            scan(&parsed, file.test_code),
        ));
    }
    let problems = problems(CATALOGUE, &entries(&catalogue), &files);
    if problems.is_empty() {
        println!("menu text: every string comes from {CATALOGUE}");
        Ok(())
    } else {
        Err(Error::Check {
            check: "menu text",
            problems,
        })
    }
}

#[cfg(test)]
mod tests;
