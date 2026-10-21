//! Small pieces of Perl's behaviour that `generate-zbm` relies on: path names, splitting, `%current`
//! substitutions, number formatting and `Data::Dumper` output.

use std::collections::BTreeMap;

/// `s{(.)/*\z}{$1}s`: removes trailing slashes, keeping a lone `/`.
fn strip_trailing_slashes(path: &str) -> &str {
    let stripped = path.trim_end_matches('/');
    if stripped.is_empty() && !path.is_empty() {
        "/"
    } else {
        stripped
    }
}

/// File::Basename's `fileparse` without suffixes: the directory part, ending with `/` (or `./`), and the rest.
fn fileparse(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(slash) => (&path[..=slash], &path[slash + 1..]),
        None => ("./", path),
    }
}

/// File::Basename's `basename` on Unix.
pub fn basename(path: &str) -> String {
    let path = strip_trailing_slashes(path);
    let (dir, base) = fileparse(path);
    if base.is_empty() {
        dir.to_owned()
    } else {
        base.to_owned()
    }
}

/// File::Basename's `dirname` on Unix.
pub fn dirname(path: &str) -> String {
    let (dir, base) = fileparse(path);
    let dir = strip_trailing_slashes(dir);
    if base.is_empty() {
        let (dir, _) = fileparse(dir);
        strip_trailing_slashes(dir).to_owned()
    } else {
        dir.to_owned()
    }
}

/// Perl's white space for `\s` on bytes.
pub fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// `split /\s+/, $text`: fields between runs of white space, with an empty first field when the text starts with
/// white space and without empty fields at the end.
pub fn split_whitespace(text: &[u8]) -> Vec<&[u8]> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < text.len() {
        if is_space(text[index]) {
            fields.push(&text[start..index]);
            while index < text.len() && is_space(text[index]) {
                index += 1;
            }
            start = index;
        } else {
            index += 1;
        }
    }
    fields.push(&text[start..]);
    while fields.last().is_some_and(|field| field.is_empty()) {
        fields.pop();
    }
    fields
}

/// `chomp`: removes one trailing newline.
pub fn chomp(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n").unwrap_or(line)
}

/// Whether a character is a word character for `\b`.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The byte positions where `needle` occurs in `text`, ignoring the case of ASCII letters.
fn find_ignoring_case(text: &str, needle: &str) -> impl Iterator<Item = usize> {
    let haystack = text.as_bytes().to_vec();
    let needle = needle.as_bytes().to_vec();
    (0..=haystack.len().saturating_sub(needle.len())).filter(move |&at| {
        haystack.len() >= needle.len()
            && haystack[at..at + needle.len()].eq_ignore_ascii_case(&needle)
    })
}

/// `s/%current\b/%{current}/i`.
pub fn mark_current(text: &str) -> String {
    let found = find_ignoring_case(text, "%current").find(|&at| {
        text.is_char_boundary(at) && !text[at + 8..].chars().next().is_some_and(is_word)
    });
    match found {
        Some(at) => format!("{}%{{current}}{}", &text[..at], &text[at + 8..]),
        None => text.to_owned(),
    }
}

/// Whether the text contains `%{current}`, in any case.
pub fn has_current(text: &str) -> bool {
    find_ignoring_case(text, "%{current}").next().is_some()
}

/// `s/%\{current\}/$replacement/i`.
pub fn replace_current(text: &str, replacement: &str) -> String {
    match find_ignoring_case(text, "%{current}").find(|&at| text.is_char_boundary(at)) {
        Some(at) => format!("{}{replacement}{}", &text[..at], &text[at + 10..]),
        None => text.to_owned(),
    }
}

/// `s/-\Q$version\E$//`: removes `-<version>` from the end, or from before a final newline.
pub fn strip_version_suffix(name: &str, version: &str) -> String {
    let suffix = format!("-{version}");
    if let Some(body) = name.strip_suffix(&format!("{suffix}\n")) {
        return format!("{body}\n");
    }
    name.strip_suffix(&suffix).unwrap_or(name).to_owned()
}

/// `sprintf("%08x", $text)`: the number at the start of the text in hex, as Perl converts a string to an unsigned
/// integer for `%x`.
pub fn hex_of_numeric_text(text: &str) -> String {
    let number = crate::config::numeric_prefix(text);
    let value = if number < 0.0 {
        // Perl converts a negative number to the unsigned integer with the same bits.
        u64::from_ne_bytes((number as i64).to_ne_bytes())
    } else {
        number as u64
    };
    format!("{value:08x}")
}

/// A string in single quotes, as Data::Dumper writes it.
fn dumper_quote(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// `Dumper($groups)` with `Indent = 1` and `Sortkeys = 1`, for a hash of arrays of file names.
pub fn dump_groups(groups: &BTreeMap<String, Vec<String>>) -> String {
    if groups.is_empty() {
        return "$VAR1 = {};\n".to_owned();
    }
    let entries: Vec<String> = groups
        .iter()
        .map(|(key, files)| {
            if files.is_empty() {
                format!("  {} => []", dumper_quote(key))
            } else {
                let items: Vec<String> = files
                    .iter()
                    .map(|file| format!("    {}", dumper_quote(file)))
                    .collect();
                format!("  {} => [\n{}\n  ]", dumper_quote(key), items.join(",\n"))
            }
        })
        .collect();
    format!("$VAR1 = {{\n{}\n}};\n", entries.join(",\n"))
}

/// Decodes text as Perl's `:encoding(UTF-8)` layer does by default: bytes that are not valid UTF-8 become `\xHH`.
pub fn decode_utf8_perlqq(bytes: &[u8]) -> String {
    let mut text = String::new();
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                text.push_str(valid);
                return text;
            }
            Err(error) => {
                let (valid, after) = rest.split_at(error.valid_up_to());
                text.push_str(std::str::from_utf8(valid).unwrap_or_default());
                let bad = error.error_len().unwrap_or(after.len()).max(1);
                for byte in &after[..bad] {
                    text.push_str(&format!("\\x{byte:02X}"));
                }
                rest = &after[bad..];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testutil::legacy::perl;
    use zbm_testutil::{Rng, have_program};

    #[test]
    fn paths() {
        assert_eq!(basename("/boot/vmlinuz-6.1"), "vmlinuz-6.1");
        assert_eq!(basename("a//"), "a");
        assert_eq!(basename("/"), "/");
        assert_eq!(dirname("/a/b/c"), "/a/b");
        assert_eq!(dirname("/a"), "/");
        assert_eq!(dirname("a"), ".");
        assert_eq!(dirname("a/b/"), "a");
    }

    #[test]
    fn text() {
        assert_eq!(
            split_whitespace(b"  7 .reloc 0000000c\n"),
            [b"".as_slice(), b"7", b".reloc", b"0000000c"]
        );
        assert_eq!(mark_current("%CURRENT-1"), "%{current}-1");
        assert_eq!(mark_current("%currentx"), "%currentx");
        assert_eq!(replace_current("v%{Current}", "3"), "v3");
        assert!(has_current("%{CURRENT}"));
        assert_eq!(strip_version_suffix("vmlinuz-6.1", "6.1"), "vmlinuz");
        assert_eq!(hex_of_numeric_text("305419896\n"), "12345678");
        assert_eq!(hex_of_numeric_text("-1"), "ffffffffffffffff");
        assert_eq!(hex_of_numeric_text(""), "00000000");
        assert_eq!(decode_utf8_perlqq(b"a\xffb"), "a\\xFFb");
    }

    #[test]
    fn dumper_matches_perl() {
        if !have_program("perl") {
            eprintln!("skipped: perl is missing");
            return;
        }
        let pieces = ["a", "/b", "3.1.0", "'", "\\", "_1", " ", "x"];
        let mut rng = Rng::from_env(12);
        for _ in 0..30 {
            let mut groups = BTreeMap::new();
            for _ in 0..rng.below(4) {
                let files: Vec<String> = (0..1 + rng.below(3))
                    .map(|_| rng.join(&pieces, 4))
                    .collect();
                groups.insert(rng.join(&pieces, 3), files);
            }
            let mut args = Vec::new();
            for (key, files) in &groups {
                args.push(key.clone());
                args.push(files.len().to_string());
                args.extend(files.iter().cloned());
            }
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let program = "use Data::Dumper; $Data::Dumper::Indent = 1; $Data::Dumper::Sortkeys = 1; \
                           $Data::Dumper::Purity = 1; my %g; while (@ARGV) { my $k = shift; my $n = shift; \
                           $g{$k} = [splice(@ARGV, 0, $n)]; } print Dumper(\\%g);";
            let output = perl(program, &arg_refs, b"");
            assert_eq!(
                dump_groups(&groups),
                output.stdout_text(),
                "groups {groups:?}"
            );
        }
    }
}
