//! The shell commands of a section of the reStructuredText documentation,
//! so that tests run the documented procedures as written.
//!
//! A section starts at a title: a line that is not blank, followed by an
//! underline of one punctuation character (`=`, `-`, `~`, `^`, `"`, `'`,
//! `` ` ``, `#`, `*` or `+`) repeated at least four times and at least as
//! long as the title. It ends at the next title.
//!
//! In a `.. code-block:: bash` block (or `sh`, `shell`), a command starts
//! with `# ` (the root prompt); following lines indented deeper than the
//! prompt continue it, lines at the prompt's indentation are example output
//! and are skipped. A block without any prompt (a function definition, an
//! array assignment) is one command.
//!
//! The text is read as the Python harness's `docs.py` read it, so that the
//! commands stayed the same: lines split as `str.splitlines` splits them,
//! indentation counted in characters.

use std::fs;
use std::path::Path;

use crate::error::{Error, Result};

/// The characters an underline may be made of.
const UNDERLINE_CHARS: &str = "=-~^\"'`#*+";

/// Whether `c` is white space as Python's `str.isspace` says.
fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// `line` without white space at either end, as Python's `str.strip`.
fn strip(line: &str) -> &str {
    line.trim_matches(is_space)
}

/// The number of white space characters that start `line`.
fn indent(line: &str) -> usize {
    line.chars().take_while(|&c| is_space(c)).count()
}

/// The lines of `text`, as Python's `str.splitlines` splits them.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let breaks = matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !breaks {
            continue;
        }
        lines.push(&text[start..index]);
        start = index + c.len_utf8();
        if c == '\r' && chars.peek().is_some_and(|&(_, next)| next == '\n') {
            chars.next();
            start += 1;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Whether `line` underlines a title: one punctuation character at least
/// four times, then only white space.
fn is_underline(line: &str) -> bool {
    let Some(first) = line.chars().next() else {
        return false;
    };
    if !UNDERLINE_CHARS.contains(first) {
        return false;
    }
    let rest = &line[first.len_utf8()..];
    let same = rest.chars().take_while(|&c| c == first).count();
    same >= 3 && rest.chars().skip(same).all(is_space)
}

/// Whether line `index` of `lines` is a title, underlined by the next one.
fn is_title(lines: &[&str], index: usize) -> bool {
    let (Some(line), Some(next)) = (lines.get(index), lines.get(index + 1)) else {
        return false;
    };
    let title = strip(line);
    !title.is_empty() && is_underline(next) && strip(next).chars().count() >= title.chars().count()
}

/// Sets `title` to `text` in `out`, in the place of an earlier section of
/// that title if there is one, as a Python dictionary does.
fn insert(out: &mut Vec<(String, String)>, title: String, text: String) {
    match out.iter_mut().find(|(known, _)| *known == title) {
        Some(entry) => entry.1 = text,
        None => out.push((title, text)),
    }
}

/// Every section of an RST text: its title and its text, in order. When
/// two sections have the same title, the later one's text is kept.
pub fn sections(text: &str) -> Vec<(String, String)> {
    let lines = split_lines(text);
    let mut out = Vec::new();
    let mut title: Option<String> = None;
    let mut buf: Vec<&str> = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if is_title(&lines, index) {
            if let Some(title) = title.take() {
                insert(&mut out, title, buf.join("\n"));
            }
            title = Some(strip(lines[index]).to_owned());
            buf.clear();
            index += 2;
            continue;
        }
        buf.push(lines[index]);
        index += 1;
    }
    if let Some(title) = title {
        insert(&mut out, title, buf.join("\n"));
    }
    out
}

/// The indentation of a `.. code-block:: bash` (or `sh`, `shell`)
/// directive on `line`, if it is one.
fn code_block(line: &str) -> Option<usize> {
    let base = indent(line);
    let directive: String = line.chars().skip(base).collect();
    let language = directive.strip_prefix(".. code-block:: ")?;
    matches!(language.trim_end_matches(is_space), "bash" | "sh" | "shell").then_some(base)
}

/// The lines of a block, without blank lines at either end, dedented to
/// the least indented line.
fn dedent(mut block: Vec<&str>) -> Vec<String> {
    while block.last().is_some_and(|line| strip(line).is_empty()) {
        block.pop();
    }
    let leading = block
        .iter()
        .take_while(|line| strip(line).is_empty())
        .count();
    block.drain(..leading);
    let Some(least) = block
        .iter()
        .filter(|line| !strip(line).is_empty())
        .map(|line| indent(line))
        .min()
    else {
        return Vec::new();
    };
    block
        .iter()
        .map(|line| line.chars().skip(least).collect())
        .collect()
}

/// The shell code blocks of a section's text: their lines, dedented. A
/// block ends at the first line that is not blank and is indented no
/// deeper than its directive.
pub fn blocks(text: &str) -> Vec<Vec<String>> {
    let lines = split_lines(text);
    let mut out = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(base) = code_block(lines[index]) else {
            index += 1;
            continue;
        };
        index += 1;
        let mut block = Vec::new();
        while let Some(line) = lines.get(index) {
            if !strip(line).is_empty() && indent(line) <= base {
                break;
            }
            block.push(*line);
            index += 1;
        }
        let block = dedent(block);
        if !block.is_empty() {
            out.push(block);
        }
    }
    out
}

/// The commands of one block, in order.
pub fn block_commands(block: &[String]) -> Vec<String> {
    if !block.iter().any(|line| line.starts_with("# ")) {
        return vec![block.join("\n")];
    }
    let mut out = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in block {
        if let Some(command) = line.strip_prefix("# ") {
            if let Some(done) = current.replace(vec![command]) {
                out.push(done.join("\n"));
            }
        } else if let Some(command) = &mut current {
            if !strip(line).is_empty() && line.starts_with(' ') {
                command.push(strip(line));
            } else {
                out.push(command.join("\n"));
                current = None;
            }
        }
    }
    if let Some(done) = current {
        out.push(done.join("\n"));
    }
    out
}

/// The shell commands of the section `title` of the RST text `text`, in
/// order; `None` when there is no such section.
pub fn section_commands(text: &str, title: &str) -> Option<Vec<String>> {
    let sections = sections(text);
    let (_, section) = sections.iter().find(|(known, _)| known == title)?;
    Some(
        blocks(section)
            .iter()
            .flat_map(|block| block_commands(block))
            .collect(),
    )
}

/// The shell commands of the section `title` of the RST file `path`, in
/// order.
///
/// # Errors
///
/// [`Error::Io`] when the file cannot be read, [`Error::Setup`] when it
/// has no such section.
pub fn commands(path: &Path, title: &str) -> Result<Vec<String>> {
    let text =
        fs::read_to_string(path).map_err(Error::io(format!("reading {}", path.display())))?;
    section_commands(&text, title)
        .ok_or_else(|| Error::Setup(format!("no section '{title}' in {}", path.display())))
}

#[cfg(test)]
mod tests;
