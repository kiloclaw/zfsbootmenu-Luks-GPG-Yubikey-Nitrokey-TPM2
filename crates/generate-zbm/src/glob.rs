//! File name globbing, as Perl's `glob` works in `generate-zbm`, which imports
//! `File::Glob qw(:globally :nocase)`.
//!
//! That makes every `glob` a call of File::Glob's `csh_glob` with the flags
//! `GLOB_BRACE`, `GLOB_NOMAGIC`, `GLOB_QUOTE`, `GLOB_TILDE`, `GLOB_ALPHASORT`
//! and `GLOB_NOCASE`:
//!
//! - the pattern is split into words at white space, except inside quotes or
//!   after a backslash;
//! - each word is expanded with braces (`{a,b}`), a leading `~`, backslash
//!   escapes, and `*`, `?` and `[...]`, which match letters in either case;
//! - a word without wildcards is returned as it is, whether or not the file
//!   exists;
//! - the matches of each word are sorted by name, ignoring case.
//!
//! The port follows Perl's `bsd_glob.c` and `Glob.xs` closely, including their
//! quirks: an unbalanced `{` globs only the part of the word before it, and an
//! unmatched quote makes the whole pattern one word. Names that are not valid
//! UTF-8 are skipped.

use std::ffi::{OsStr, c_char};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Expands `pattern` as `glob` does in `generate-zbm`.
pub fn glob(pattern: &str) -> Vec<String> {
    glob_at(pattern, Path::new(""))
}

/// Expands `pattern` with relative paths looked up from `base` instead of the
/// working directory; the results are the same strings `glob` would give when
/// run from `base`.
fn glob_at(pattern: &str, base: &Path) -> Vec<String> {
    let mut results = Vec::new();
    for word in split_words(pattern.as_bytes()) {
        let mut matches = Vec::new();
        expand_braces(&protect(&word), base, &mut matches);
        results.extend(
            matches
                .into_iter()
                .filter_map(|bytes| String::from_utf8(bytes).ok()),
        );
    }
    results
}

/// Perl's `isSPACE` for ASCII.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c')
}

/// Splits a pattern into words as `csh_glob` does: at white space outside
/// quotes; quotes are removed, and a backslash before a quote is removed while
/// other backslashes are kept for the escapes of `bsd_glob`.
fn split_words(pattern: &[u8]) -> Vec<Vec<u8>> {
    let end = pattern.len();
    let mut words = Vec::new();
    let mut word: Option<Vec<u8>> = None;
    let mut piece: Option<usize> = None;
    let mut s = 0;
    while s < end {
        match pattern[s] {
            quote @ (b'\'' | b'"') => {
                let current = word.get_or_insert_with(Vec::new);
                if let Some(start) = piece {
                    current.extend_from_slice(&pattern[start..s]);
                }
                let mut start = s + 1;
                let mut found = false;
                loop {
                    s += 1;
                    if s >= end {
                        break;
                    }
                    if pattern[s] == b'\\' {
                        s += 1;
                        if s < end && pattern[s] == quote {
                            current.extend_from_slice(&pattern[start..s - 1]);
                            start = s;
                        }
                    } else if pattern[s] == quote {
                        found = true;
                        break;
                    }
                }
                if !found {
                    // An unmatched quote: the whole pattern, without surrounding white space, is
                    // the last word.
                    let first = pattern
                        .iter()
                        .position(|byte| !is_space(*byte))
                        .unwrap_or(end);
                    let last = pattern
                        .iter()
                        .rposition(|byte| !is_space(*byte))
                        .map_or(first, |i| i + 1);
                    words.push(pattern[first..last].to_vec());
                    return words;
                }
                current.extend_from_slice(&pattern[start..s]);
                piece = None;
            }
            b'\\' => {
                let start = *piece.get_or_insert(s);
                s += 1;
                if s < end && (pattern[s] == b'\'' || pattern[s] == b'"') {
                    word.get_or_insert_with(Vec::new)
                        .extend_from_slice(&pattern[start..s - 1]);
                    piece = Some(s);
                }
            }
            byte if is_space(byte) => {
                if let Some(start) = piece.take() {
                    word.get_or_insert_with(Vec::new)
                        .extend_from_slice(&pattern[start..s]);
                }
                if let Some(done) = word.take() {
                    words.push(done);
                }
            }
            _ => {
                piece.get_or_insert(s);
            }
        }
        s += 1;
    }
    if piece.is_some() || word.is_some() {
        let mut last = word.unwrap_or_default();
        if let Some(start) = piece {
            last.extend_from_slice(&pattern[start..end.min(s)]);
        }
        words.push(last);
    }
    words
}

/// A pattern character and whether a backslash protects it from being special.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Char {
    byte: u8,
    protected: bool,
}

impl Char {
    fn is(self, byte: u8) -> bool {
        !self.protected && self.byte == byte
    }
}

/// `GLOB_QUOTE`: a backslash protects the next character; a final backslash
/// stands for itself.
fn protect(word: &[u8]) -> Vec<Char> {
    let mut chars = Vec::with_capacity(word.len());
    let mut bytes = word.iter().copied().peekable();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' {
            match bytes.next() {
                Some(next) => chars.push(Char {
                    byte: next,
                    protected: true,
                }),
                None => chars.push(Char {
                    byte: b'\\',
                    protected: true,
                }),
            }
        } else {
            chars.push(Char {
                byte,
                protected: false,
            });
        }
    }
    chars
}

/// `globexp1`: expands the first brace group, recursively, and globs each
/// result.
fn expand_braces(pattern: &[Char], base: &Path, matches: &mut Vec<Vec<u8>>) {
    // A lone `{}` is kept, for find(1), as csh does.
    if pattern.len() == 2 && pattern[0].is(b'{') && pattern[1].is(b'}') {
        glob0(pattern, base, matches);
        return;
    }
    match pattern.iter().position(|c| c.is(b'{')) {
        Some(brace) => expand_brace_at(pattern, brace, base, matches),
        None => glob0(pattern, base, matches),
    }
}

/// The index of the `]` that closes a bracket opened at `open`, if there is
/// one.
fn closing_bracket(pattern: &[Char], open: usize) -> Option<usize> {
    (open + 1..pattern.len()).find(|&index| pattern[index].is(b']'))
}

/// `globexp2`: expands the brace group that starts at `brace`.
fn expand_brace_at(pattern: &[Char], brace: usize, base: &Path, matches: &mut Vec<Vec<u8>>) {
    let prefix = &pattern[..brace];
    // Find the balanced closing brace, skipping bracket expressions.
    let mut depth = 0;
    let mut close = None;
    let mut index = brace + 1;
    while index < pattern.len() {
        let c = pattern[index];
        if c.is(b'[') {
            if let Some(end) = closing_bracket(pattern, index) {
                index = end;
            }
        } else if c.is(b'{') {
            depth += 1;
        } else if c.is(b'}') {
            if depth == 0 {
                close = Some(index);
                break;
            }
            depth -= 1;
        }
        index += 1;
    }
    let Some(close) = close else {
        // Unbalanced: bsd_glob globs only what precedes the brace.
        glob0(prefix, base, matches);
        return;
    };
    let suffix = &pattern[close + 1..];
    let mut depth = 0;
    let mut start = brace + 1;
    let mut index = brace + 1;
    while index <= close {
        let c = pattern[index];
        if c.is(b'[') {
            if let Some(end) = closing_bracket(pattern, index) {
                index = end;
            }
        } else if c.is(b'{') {
            depth += 1;
        } else if c.is(b'}') && depth > 0 {
            depth -= 1;
        } else if (c.is(b'}') || c.is(b',')) && depth == 0 {
            let mut expanded = prefix.to_vec();
            expanded.extend_from_slice(&pattern[start..index]);
            expanded.extend_from_slice(suffix);
            expand_braces(&expanded, base, matches);
            start = index + 1;
        }
        index += 1;
    }
}

/// A compiled pattern element, as `glob0` builds them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Token {
    Byte(u8),
    /// `*`
    All,
    /// `?`
    One,
    /// The start of a bracket expression.
    Set,
    /// `!` at the start of a bracket expression.
    Not,
    /// A range inside a bracket expression; the bytes on either side are its ends.
    Range,
    /// The end of a bracket expression.
    End,
}

impl Token {
    fn is_meta(self) -> bool {
        !matches!(self, Token::Byte(_))
    }
}

/// `glob0`: expands a leading `~`, compiles the pattern and globs it. A pattern
/// without wildcards that matches nothing is returned as it is; matches are
/// sorted.
fn glob0(pattern: &[Char], base: &Path, matches: &mut Vec<Vec<u8>>) {
    let pattern = expand_tilde(pattern);
    let mut tokens = Vec::new();
    let mut magic = false;
    let mut index = 0;
    while index < pattern.len() {
        let c = pattern[index];
        index += 1;
        if c.is(b'[') {
            let negate = pattern.get(index).is_some_and(|next| next.is(b'!'));
            let body = if negate { index + 1 } else { index };
            let has_close =
                body < pattern.len() && (body + 1..pattern.len()).any(|i| pattern[i].is(b']'));
            if !has_close {
                tokens.push(Token::Byte(b'['));
                continue;
            }
            tokens.push(Token::Set);
            if negate {
                tokens.push(Token::Not);
            }
            index = body;
            let mut c = pattern[index];
            index += 1;
            loop {
                tokens.push(Token::Byte(c.byte));
                if pattern.get(index).is_some_and(|next| next.is(b'-'))
                    && let Some(&high) = pattern.get(index + 1)
                    && !high.is(b']')
                {
                    tokens.push(Token::Range);
                    tokens.push(Token::Byte(high.byte));
                    index += 2;
                }
                c = pattern[index];
                index += 1;
                if c.is(b']') {
                    break;
                }
            }
            magic = true;
            tokens.push(Token::End);
        } else if c.is(b'?') {
            magic = true;
            tokens.push(Token::One);
        } else if c.is(b'*') {
            magic = true;
            if tokens.last() != Some(&Token::All) {
                tokens.push(Token::All);
            }
        } else {
            tokens.push(Token::Byte(c.byte));
        }
    }

    let before = matches.len();
    if !tokens.is_empty() {
        glob2(&mut Vec::new(), &tokens, base, matches);
    }
    if matches.len() == before {
        if !magic {
            matches.push(pattern.iter().map(|c| c.byte).collect());
        }
    } else {
        matches[before..].sort_by(|a, b| compare_ignoring_case(a, b));
    }
}

/// `globtilde`: replaces a leading `~` or `~user` with the home directory.
fn expand_tilde(pattern: &[Char]) -> Vec<Char> {
    if !pattern.first().is_some_and(|c| c.is(b'~')) {
        return pattern.to_vec();
    }
    let name_end = pattern
        .iter()
        .position(|c| c.is(b'/'))
        .unwrap_or(pattern.len());
    let name: Vec<u8> = pattern[1..name_end].iter().map(|c| c.byte).collect();
    let home = if name.is_empty() {
        std::env::var_os("HOME")
            .map(|home| home.as_bytes().to_vec())
            .or_else(|| home_of(|entry| entry.uid == zbm_sys::credentials().uid))
    } else {
        home_of(|entry| entry.name == name)
    };
    let Some(home) = home else {
        return pattern.to_vec();
    };
    let mut expanded: Vec<Char> = home
        .into_iter()
        .map(|byte| Char {
            byte,
            protected: false,
        })
        .collect();
    expanded.extend_from_slice(&pattern[name_end..]);
    expanded
}

struct PasswdEntry {
    name: Vec<u8>,
    uid: u32,
    home: Vec<u8>,
}

/// The home directory of the first `/etc/passwd` entry that `wanted` accepts.
fn home_of(wanted: impl Fn(&PasswdEntry) -> bool) -> Option<Vec<u8>> {
    let passwd = std::fs::read("/etc/passwd").ok()?;
    passwd.split(|byte| *byte == b'\n').find_map(|line| {
        let fields: Vec<&[u8]> = line.split(|byte| *byte == b':').collect();
        if fields.len() < 6 {
            return None;
        }
        let entry = PasswdEntry {
            name: fields[0].to_vec(),
            uid: std::str::from_utf8(fields[2]).ok()?.parse().ok()?,
            home: fields[5].to_vec(),
        };
        wanted(&entry).then_some(entry.home)
    })
}

/// `glob2`: walks the pattern one path segment at a time; segments without
/// wildcards are appended as they are.
fn glob2(path: &mut Vec<u8>, pattern: &[Token], base: &Path, matches: &mut Vec<Vec<u8>>) {
    let mut rest = pattern;
    loop {
        if rest.is_empty() {
            if std::fs::symlink_metadata(base.join(OsStr::from_bytes(path))).is_ok() {
                matches.push(path.clone());
            }
            return;
        }
        let segment_end = rest
            .iter()
            .position(|token| *token == Token::Byte(b'/'))
            .unwrap_or(rest.len());
        let segment = &rest[..segment_end];
        if segment.iter().any(|token| token.is_meta()) {
            glob3(path, segment, &rest[segment_end..], base, matches);
            return;
        }
        path.extend(segment.iter().map(|token| match token {
            Token::Byte(byte) => *byte,
            _ => unreachable!("the segment has no wildcards"),
        }));
        let mut after = segment_end;
        while rest.get(after) == Some(&Token::Byte(b'/')) {
            path.push(b'/');
            after += 1;
        }
        rest = &rest[after..];
    }
}

/// `glob3`: matches one segment with wildcards against the entries of the
/// directory reached so far.
fn glob3(path: &[u8], segment: &[Token], rest: &[Token], base: &Path, matches: &mut Vec<Vec<u8>>) {
    let dir = if path.is_empty() {
        b".".as_slice()
    } else {
        path
    };
    let Ok(entries) = std::fs::read_dir(base.join(OsStr::from_bytes(dir))) else {
        return;
    };
    // readdir also returns `.` and `..`, which `.*` matches.
    let mut names: Vec<Vec<u8>> = vec![b".".to_vec(), b"..".to_vec()];
    names.extend(
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().as_bytes().to_vec()),
    );
    for name in names {
        // A leading dot must be matched literally.
        if name.first() == Some(&b'.') && segment.first() != Some(&Token::Byte(b'.')) {
            continue;
        }
        if !matches_segment(&name, segment) {
            continue;
        }
        let mut next = path.to_vec();
        next.extend_from_slice(&name);
        glob2(&mut next, rest, base, matches);
    }
}

/// `match` with `GLOB_NOCASE`: whether `name` matches the segment, ignoring the
/// case of ASCII letters.
fn matches_segment(name: &[u8], pattern: &[Token]) -> bool {
    let lower = |byte: u8| byte.to_ascii_lowercase();
    let Some((&token, rest)) = pattern.split_first() else {
        return name.is_empty();
    };
    match token {
        Token::All => {
            if rest.is_empty() {
                return true;
            }
            (0..=name.len()).any(|skip| matches_segment(&name[skip..], rest))
        }
        Token::One => !name.is_empty() && matches_segment(&name[1..], rest),
        Token::Set => {
            let Some((&k, name_rest)) = name.split_first() else {
                return false;
            };
            let mut items = rest;
            let negate = items.first() == Some(&Token::Not);
            if negate {
                items = &items[1..];
            }
            let mut ok = false;
            loop {
                let Some((&item, after)) = items.split_first() else {
                    return false;
                };
                items = after;
                let Token::Byte(low) = item else {
                    // `End`.
                    break;
                };
                if items.first() == Some(&Token::Range) {
                    if let Some(Token::Byte(high)) = items.get(1)
                        && lower(low) <= lower(k)
                        && lower(k) <= lower(*high)
                    {
                        ok = true;
                    }
                    items = &items[2..];
                } else if lower(low) == lower(k) {
                    ok = true;
                }
            }
            ok != negate && matches_segment(name_rest, items)
        }
        Token::Byte(c) => name.split_first().is_some_and(|(&k, name_rest)| {
            lower(k) == lower(c) && matches_segment(name_rest, rest)
        }),
        Token::Not | Token::Range | Token::End => false,
    }
}

/// `ci_compare`: compares names by their lower-case letters, as C `char`s, and
/// then exactly.
fn compare_ignoring_case(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    // The bytes are compared as C `char`s, which are signed on some machines.
    let fold = |byte: Option<&u8>| -> i32 {
        byte.map_or(0, |byte| i32::from(byte.to_ascii_lowercase() as c_char))
    };
    let mut index = 0;
    while index < a.len() && index < b.len() && a[index].eq_ignore_ascii_case(&b[index]) {
        index += 1;
    }
    match fold(a.get(index)).cmp(&fold(b.get(index))) {
        std::cmp::Ordering::Equal => a.cmp(b),
        order => order,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testutil::legacy::perl;
    use zbm_testutil::{Rng, TempDir, have_program};

    #[test]
    fn words() {
        let words = |pattern: &str| -> Vec<String> {
            split_words(pattern.as_bytes())
                .into_iter()
                .map(|word| String::from_utf8(word).unwrap())
                .collect()
        };
        assert_eq!(words("a b"), ["a", "b"]);
        assert_eq!(words(" \"d e\" x\\ y"), ["d e", "x\\ y"]);
        assert_eq!(words("a \"b"), ["a", "a \"b"]);
        assert_eq!(words("\"\""), [""]);
        assert_eq!(words("\\\"b"), ["\"b"]);
    }

    #[test]
    fn files() {
        let dir = TempDir::new("generate-zbm-glob");
        for name in ["Ab", "ab", "aB", ".h", "b", "x[1]"] {
            dir.write(name, "");
        }
        let base = dir.path().display().to_string();
        let names = |pattern: &str| -> Vec<String> {
            glob(&format!("{base}/{pattern}"))
                .into_iter()
                .map(|path| path[base.len() + 1..].to_owned())
                .collect()
        };
        assert_eq!(names("*"), ["Ab", "aB", "ab", "b", "x[1]"]);
        assert_eq!(names("A?"), ["Ab", "aB", "ab"]);
        assert_eq!(names(".*"), [".", "..", ".h"]);
        assert_eq!(names("[!a]*"), ["b", "x[1]"]);
        assert_eq!(names("x\\[1]"), ["x[1]"]);
        assert_eq!(names("{b,zz}"), ["b", "zz"]);
        assert_eq!(names("none*"), Vec::<String>::new());
        assert_eq!(glob("a{b"), ["a"]);
        assert_eq!(glob("\\*"), ["*"]);
    }

    fn perl_glob(pattern: &str, cwd: &std::path::Path) -> String {
        let program = "use File::Glob qw(:globally :nocase); chdir $ARGV[1] or die;\n\
                       print map { \"<$_>\" } glob($ARGV[0]);";
        perl(program, &[pattern, &cwd.to_string_lossy()], b"").stdout_text()
    }

    #[test]
    fn matches_file_glob() {
        if !have_program("perl") {
            eprintln!("skipped: perl is missing");
            return;
        }
        let dir = TempDir::new("generate-zbm-glob-parity");
        for name in [
            "vmlinuz-6.1.0",
            "VMLINUZ-6.2",
            "vmlinuz-6.10_1.EFI",
            "Ab",
            "ab",
            ".hidden",
            "d e",
            "x[1]",
            "a{b",
            "q'x",
            "-",
            "]",
            "sub/f",
            "sub/G",
            "sub/.dot",
            "s2/f",
        ] {
            dir.write(name, "");
        }
        let pieces = [
            "*", "?", "[ab]", "[!a]", "[]a]", "[a-z]", "[A-Z]", "[a-", "[", "]", "{", "}", ",",
            "{a,b}", "{,}", "\\", "\\*", "'", "\"", " ", "  ", "/", "sub", "s*", "vmlinuz", "-",
            "6", ".", ".*", "EFI", "_", "x", "d", "e", "~", "q", "{}", "1",
        ];
        let mut rng = Rng::from_env(11);
        for _ in 0..400 {
            let pattern = rng.join(&pieces, 5);
            let rust: String = glob_at(&pattern, dir.path())
                .iter()
                .map(|path| format!("<{path}>"))
                .collect();
            assert_eq!(rust, perl_glob(&pattern, dir.path()), "pattern {pattern:?}");
        }
    }
}
