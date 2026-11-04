//! The shell the runtime runs its scripts, the hooks and the recovery shell
//! with. Images with the Rust runtime have brush ([`BRUSH`]), a
//! Bash-compatible shell in Rust; GNU Bash ([`BASH`]) stays in every image,
//! as dracut's `/bin/sh`. `zfsbootmenu-preinit` chooses the shell once, from
//! `zbm.shell` and what the image has ([`choose`]), and passes it on to every
//! program of the runtime in the environment ([`ENV`]), which [`chosen`]
//! reads.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Where images with the Rust runtime install brush.
pub const BRUSH: &str = "/usr/bin/brush";

/// GNU Bash.
pub const BASH: &str = "/bin/bash";

/// The environment variable that names the shell for the runtime's programs.
pub const ENV: &str = "ZBM_SHELL";

/// The shell `zfsbootmenu-preinit` chooses: brush when the image has it
/// (`has_brush`) and `wanted`, the value of `zbm.shell`, is not `bash`;
/// GNU Bash otherwise.
pub fn choose(wanted: Option<&str>, has_brush: bool) -> &'static str {
    if has_brush && wanted != Some("bash") {
        BRUSH
    } else {
        BASH
    }
}

/// The shell named by `value`, the environment's [`ENV`], when it is an
/// absolute path.
pub fn from_env(value: Option<&OsStr>) -> Option<PathBuf> {
    value
        .map(Path::new)
        .filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
}

/// The shell `zfsbootmenu-preinit` chose for this process and the programs
/// it starts; `None` where it chose none (legacy images, tests), and each
/// caller keeps the Bash it ran before.
pub fn chosen() -> Option<PathBuf> {
    use crate::env::Environment as _;
    from_env(crate::env::ProcessEnvironment.var_os(ENV).as_deref())
}

/// How a hook or script runs, told from its first bytes before it starts
/// (ADR 0017, point 6): the C library's `execvp`, which runs a file the
/// kernel refuses on `/bin/sh`, would hide that it was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Interpreter {
    /// A `#!` line naming `sh` or `bash` directly, with what follows the
    /// interpreter as the kernel passes it: one argument, if any.
    Shell(Option<String>),
    /// Another `#!` line, `env` included, or an ELF file: the kernel runs
    /// it as it is.
    Kernel,
    /// Neither: a text for the shell, as Bash ran it in the legacy runtime.
    Text,
}

/// How the file whose first bytes are `start` runs. The `#!` line is read
/// as Linux reads it: the interpreter up to the first blank, and the rest of
/// the line, without its outer blanks, as one argument.
pub fn interpreter(start: &[u8]) -> Interpreter {
    if start.starts_with(b"\x7fELF") {
        return Interpreter::Kernel;
    }
    let Some(line) = start.strip_prefix(b"#!") else {
        return Interpreter::Text;
    };
    let line = line.split(|&byte| byte == b'\n').next().unwrap_or_default();
    let line = String::from_utf8_lossy(line);
    // A line ending in `\r\n` is read without its `\r`, as the runtime
    // read it before: a script saved on another system still runs.
    let line = line.trim_matches([' ', '\t', '\r']);
    let (program, argument) = line
        .split_once([' ', '\t'])
        .map_or((line, ""), |(program, rest)| {
            (program, rest.trim_matches([' ', '\t']))
        });
    match Path::new(program).file_name().and_then(OsStr::to_str) {
        Some("sh" | "bash") => {
            Interpreter::Shell((!argument.is_empty()).then(|| argument.to_owned()))
        }
        _ => Interpreter::Kernel,
    }
}

/// Quotes `text` as one shell word, in single quotes, with each `'` written as
/// `'\''`.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Why a text is not plain shell words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WordsError {
    /// A quote that does not close.
    Unclosed(char),
    /// `$`, `` ` `` or a leading `~`: what a shell would expand.
    Expansion(char),
    /// `*`, `?` or `[`: a pattern a shell would match against files.
    Glob(char),
    /// `;`, `|`, `&`, `<`, `>`, `(`, `)` or a leading `#`: shell syntax.
    Syntax(char),
}

impl std::fmt::Display for WordsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WordsError::Unclosed(quote) => write!(formatter, "the quote {quote} does not close"),
            WordsError::Expansion(c) => write!(
                formatter,
                "{c} asks a shell to expand it, and no shell runs; quote it to keep it"
            ),
            WordsError::Glob(c) => write!(
                formatter,
                "{c} asks a shell to match files, and no shell runs; quote it to keep it"
            ),
            WordsError::Syntax(c) => write!(
                formatter,
                "{c} is shell syntax, and no shell runs; quote it to keep it"
            ),
        }
    }
}

/// What the quoted text after `"` holds up to its closing quote, from
/// `chars`: `\` keeps `$`, `` ` ``, `"`, `\\` and a newline (which it
/// removes), as the shell does.
fn double_quoted(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    word: &mut String,
) -> Result<(), WordsError> {
    loop {
        match chars.next() {
            None => return Err(WordsError::Unclosed('"')),
            Some('"') => return Ok(()),
            Some(c @ ('$' | '`')) => return Err(WordsError::Expansion(c)),
            Some('\\') => match chars.peek().copied() {
                Some('\n') => {
                    chars.next();
                }
                Some(c @ ('$' | '`' | '"' | '\\')) => {
                    chars.next();
                    word.push(c);
                }
                _ => word.push('\\'),
            },
            Some(c) => word.push(c),
        }
    }
}

/// The words of `text` as a shell splits them, quotes and backslashes taken
/// as it takes them, without anything it would expand: for the words a
/// configuration gives to a program that runs without a shell.
///
/// # Errors
///
/// For a quote that does not close, and for what only a shell gives a
/// meaning: expansions, patterns and syntax, outside quotes; refused, so
/// that the program never sees them with another meaning than the one
/// written.
pub fn split_words(text: &str) -> Result<Vec<String>, WordsError> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let starts = word.is_none();
        match c {
            ' ' | '\t' | '\n' => words.extend(word.take()),
            '\'' => {
                let current = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        None => return Err(WordsError::Unclosed('\'')),
                        Some('\'') => break,
                        Some(c) => current.push(c),
                    }
                }
            }
            '"' => double_quoted(&mut chars, word.get_or_insert_with(String::new))?,
            '\\' => match chars.next() {
                Some('\n') => {}
                Some(c) => word.get_or_insert_with(String::new).push(c),
                None => word.get_or_insert_with(String::new).push('\\'),
            },
            '$' | '`' => return Err(WordsError::Expansion(c)),
            '~' if starts => return Err(WordsError::Expansion(c)),
            '*' | '?' | '[' => return Err(WordsError::Glob(c)),
            ';' | '|' | '&' | '<' | '>' | '(' | ')' => return Err(WordsError::Syntax(c)),
            '#' if starts => return Err(WordsError::Syntax(c)),
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    words.extend(word);
    Ok(words)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn brush_unless_the_image_lacks_it_or_bash_is_asked_for() {
        assert_eq!(choose(None, true), BRUSH);
        assert_eq!(choose(Some("brush"), true), BRUSH);
        assert_eq!(choose(Some("bash"), true), BASH);
        assert_eq!(choose(None, false), BASH);
    }

    #[test]
    fn the_environment_names_the_shell_with_an_absolute_path() {
        assert_eq!(
            from_env(Some(OsStr::new(BRUSH))),
            Some(PathBuf::from(BRUSH))
        );
        assert_eq!(from_env(Some(OsStr::new("brush"))), None);
        assert_eq!(from_env(None), None);
    }

    #[test]
    fn a_file_is_told_by_its_first_bytes_as_the_kernel_reads_them() {
        let shell = |argument: Option<&str>| Interpreter::Shell(argument.map(str::to_owned));
        for (start, expected) in [
            (&b"#!/bin/bash\necho hi\n"[..], shell(None)),
            (b"#!/bin/sh -e\n", shell(Some("-e"))),
            (b"#! /bin/sh  -eu \t\nset -x\n", shell(Some("-eu"))),
            (b"#!/usr/bin/bash -o pipefail\n", shell(Some("-o pipefail"))),
            (b"#!/bin/bash\r\necho hi\r\n", shell(None)),
            (b"#!/bin/sh -e\r\n", shell(Some("-e"))),
            (b"#!/usr/bin/env bash\n", Interpreter::Kernel),
            (b"#!/usr/bin/python3\nprint()\n", Interpreter::Kernel),
            (b"#!/bin/zsh", Interpreter::Kernel),
            (b"#!", Interpreter::Kernel),
            (b"\x7fELF\x02\x01\x01\0", Interpreter::Kernel),
            (b"echo hi\n", Interpreter::Text),
            (b"", Interpreter::Text),
        ] {
            assert_eq!(interpreter(start), expected, "{start:?}");
        }
    }

    #[test]
    fn quoting_round_trips_through_sh() {
        for text in ["", "a b", "it's", "$HOME", "a\nb", "'\\'", "*"] {
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("printf %s {}", quote(text)))
                .output()
                .unwrap();
            assert_eq!(String::from_utf8(output.stdout).unwrap(), text);
        }
    }

    #[test]
    fn words_split_as_the_shell_splits_them_without_expanding() {
        let words = |text: &str| split_words(text);
        assert_eq!(
            words(
                r#"--add 'zfs crypt' -o "a \"b\" \$c \d" e\ f g\
h ''"#
            ),
            Ok(vec![
                "--add".to_owned(),
                "zfs crypt".to_owned(),
                "-o".to_owned(),
                r#"a "b" $c \d"#.to_owned(),
                "e f".to_owned(),
                "gh".to_owned(),
                String::new(),
            ])
        );
        assert_eq!(words("  "), Ok(Vec::new()));
        assert_eq!(words("a\\"), Ok(vec!["a\\".to_owned()]));
        assert_eq!(
            words(
                r#""a\
b""#
            ),
            Ok(vec!["ab".to_owned()])
        );
        let kept = vec!["a=~/x".to_owned(), "$HOME".to_owned(), "*".to_owned()];
        assert_eq!(words("a=~/x '$HOME' \\*"), Ok(kept));
        for (text, error) in [
            ("'open", WordsError::Unclosed('\'')),
            ("\"open", WordsError::Unclosed('"')),
            ("$HOME", WordsError::Expansion('$')),
            ("\"$HOME\"", WordsError::Expansion('$')),
            ("\"`id`\"", WordsError::Expansion('`')),
            ("`id`", WordsError::Expansion('`')),
            ("~/x", WordsError::Expansion('~')),
            ("*.conf", WordsError::Glob('*')),
            ("a; b", WordsError::Syntax(';')),
            ("# note", WordsError::Syntax('#')),
        ] {
            assert_eq!(words(text), Err(error), "{text}");
            assert!(!error.to_string().is_empty());
        }
        assert!(WordsError::Glob('?').to_string().contains("match files"));
    }
}
