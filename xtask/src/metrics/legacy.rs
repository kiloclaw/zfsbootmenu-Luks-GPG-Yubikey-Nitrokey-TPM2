//! The names of the functions defined by the legacy Bash and Perl code.
//!
//! A doc comment of the Rust code that names one of these explains the Rust
//! by the code it replaced, which the refactor removes.

use std::collections::HashSet;
use std::path::Path;

use crate::error::{Error, Result};

/// The shortest name that counts; shorter ones match too much prose.
const MIN_NAME: usize = 3;

/// The function names defined by the files below `dir`.
///
/// An absent directory has none, so the metric still works once the legacy
/// tree is deleted. Files that are not UTF-8 text are skipped.
///
/// # Errors
///
/// [`Error::Io`] when a directory cannot be read.
pub fn function_names(dir: &Path) -> Result<HashSet<String>> {
    let mut names = HashSet::new();
    if dir.is_dir() {
        collect(dir, &mut names)?;
    }
    Ok(names)
}

/// Adds the names defined below `dir` to `names`.
fn collect(dir: &Path, names: &mut HashSet<String>) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| Error::io(format!("reading {}", dir.display()), error))?;
    for entry in entries {
        let path = entry
            .map_err(|error| Error::io(format!("reading {}", dir.display()), error))?
            .path();
        if path.is_dir() {
            collect(&path, names)?;
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            names.extend(names_in(&text));
        }
    }
    Ok(())
}

/// The function names that `text` defines: Bash `name() {`, `name ()` and
/// `function name`, and Perl `sub name`.
pub fn names_in(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().filter_map(|line| {
        let line = line.trim_start();
        let name = if let Some(rest) = line.strip_prefix("sub ") {
            identifier(rest.trim_start())
        } else if let Some(rest) = line.strip_prefix("function ") {
            identifier(rest.trim_start())
        } else {
            let name = identifier(line)?;
            line[name.len()..]
                .trim_start()
                .starts_with("()")
                .then_some(name)
        }?;
        (name.len() >= MIN_NAME).then(|| name.to_owned())
    })
}

/// The identifier at the start of `text`, if there is one.
fn identifier(text: &str) -> Option<&str> {
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    let name = &text[..end];
    let starts_well = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    starts_well.then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> Vec<String> {
        names_in(text).collect()
    }

    #[test]
    fn bash_function_definitions_are_found() {
        let text = "luks_open() {\n  :\n}\nfunction zdebug {\n}\n  csv_cat () {\n";

        assert_eq!(names(text), ["luks_open", "zdebug", "csv_cat"]);
    }

    #[test]
    fn perl_subroutines_are_found() {
        assert_eq!(names("sub versionedKernel {\n"), ["versionedKernel"]);
    }

    #[test]
    fn calls_and_short_names_are_not_definitions() {
        let text = "luks_open \"$dev\"\nok() {\n}\nif x; then\n";

        assert_eq!(names(text), Vec::<String>::new());
    }

    #[test]
    fn a_missing_directory_has_no_names() {
        let names = function_names(Path::new("/nonexistent/legacy")).unwrap();

        assert!(names.is_empty());
    }
}
