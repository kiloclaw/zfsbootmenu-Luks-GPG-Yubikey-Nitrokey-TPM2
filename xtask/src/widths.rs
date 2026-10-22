//! The line widths of The Rust Style Guide that `rustfmt` does not enforce.
//!
//! No line is longer than 100 characters, and a line that holds only a
//! comment is no longer than 80 characters without its indentation.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// The longest a line may be.
pub const MAX_LINE: usize = 100;

/// The longest a comment line may be without its indentation.
pub const MAX_COMMENT: usize = 80;

/// The problems of one file's text, one line each, as `path:line: reason`.
pub fn check_text(path: &Path, text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let width = line.chars().count();
        let comment = line.trim_start_matches(' ');
        let comment_width = comment.chars().count();
        if width > MAX_LINE {
            problems.push(format!(
                "{}:{number}: {width} characters (at most {MAX_LINE})",
                path.display()
            ));
        } else if comment.starts_with("//") && comment_width > MAX_COMMENT {
            problems.push(format!(
                "{}:{number}: comment of {comment_width} characters without \
                 indentation (at most {MAX_COMMENT})",
                path.display()
            ));
        }
    }
    problems
}

/// The problems of every `.rs` file below `roots`, relative to `base`.
///
/// # Errors
///
/// [`Error::Io`] when a directory or file cannot be read.
pub fn check(base: &Path, roots: &[&str]) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for root in roots {
        rust_files(&base.join(root), &mut files)?;
    }
    files.sort();
    let mut problems = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file)
            .map_err(|error| Error::io(format!("reading {}", file.display()), error))?;
        let shown = file.strip_prefix(base).unwrap_or(&file);
        problems.extend(check_text(shown, &text));
    }
    Ok(problems)
}

/// Adds the `.rs` files below `dir` to `files`, skipping `target`.
fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| Error::io(format!("reading {}", dir.display()), error))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| Error::io(format!("reading {}", dir.display()), error))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| Error::io(format!("inspecting {}", path.display()), error))?;
        if file_type.is_dir() && entry.file_name() != "target" {
            rust_files(&path, files)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_of_101_characters_is_too_long() {
        let text = format!("{}\n{}\n", "x".repeat(100), "y".repeat(101));

        let problems = check_text(Path::new("a.rs"), &text);

        assert_eq!(problems, ["a.rs:2: 101 characters (at most 100)"]);
    }

    #[test]
    fn a_comment_counts_without_its_indentation() {
        let fits = format!("        {}", "/".repeat(80));
        let too_long = format!("    {}", "/".repeat(81));
        let text = format!("{fits}\n{too_long}\n");

        let problems = check_text(Path::new("a.rs"), &text);

        assert_eq!(
            problems,
            ["a.rs:2: comment of 81 characters without indentation (at most 80)"]
        );
    }

    #[test]
    fn code_after_80_characters_is_fine() {
        let text = format!("let x = {};\n", "1".repeat(80));

        assert_eq!(check_text(Path::new("a.rs"), &text), Vec::<String>::new());
    }

    #[test]
    fn width_counts_characters_not_bytes() {
        let text = format!("// {}\n", "é".repeat(77));

        assert_eq!(check_text(Path::new("a.rs"), &text), Vec::<String>::new());
    }
}
